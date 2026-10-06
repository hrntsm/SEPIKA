# Issue #419 テスト監査

作成日: 2026-10-06

対象: `hrntsm/SEPIKA` の基準コミット `3286fbcb45a3cc7e296a839a5488d3a3b42a9307`。

全テストを棚卸しし、削除・統合・簡略化の全候補を以下に記載した。全件の判定・場所・検査証跡は [全テスト判断表](Issue419_全テスト判断表.csv) を参照。行番号とソースリンクは整理前の基準コミットに固定した。

## 対象と方法

- `crates/` と `xtask/` の 254 ファイルにある `#[test]` / `#[tokio::test]` 3,052 関数を抽出した。GUI・MCP・Parquet 有効の workspace コンパイル済み一覧にも全 3,052 関数が存在し、抽出漏れを照合した。
- 追加の rustdoc 2 件も判断表に含め、対象は合計 3,054 件。ignored の手動計測 3 件も除外せず、通常回帰保証と別の役割として判定した。
- テスト名・期待値・assert/呼出し構造・同じ対象への重複を横断して確認し、候補は本文・production の対象分岐・代替するテストを照合した。名前や assert の数だけで削除判定しない。helper 経由の独立検査・snapshot・should_panic は直接 assert がなくても検査として扱う。
- 判定は「削除したらどんな現実的な不具合を見逃すか」。KEEP の全件行に対象経路のリスクと検査証跡を記載した。件数・カバレッジ維持や「念のため」は理由にしない。
- MERGE は共通 setup・契約を共有する提案。異なる backend / command の branch や規準表の値を消す意味ではない。テスト名だけ減らすために関係ない body を連結しない。SIMPLIFY に代替保証の移設がある場合、それを済ませてから旧確認を削る。

## 判定数

| 判定 | 対象数 | 意味 |
|---|---:|---|
| KEEP | 2,756 | 独立した物理/外部入力/状態/解析経路の不具合を検出 |
| DELETE | 33 | 固有保証なし、または強い既存テストと重複 |
| MERGE | 219 | 必要な条件を保持して同一契約・fixture を共有 |
| SIMPLIFY | 46 | 不要な反復/内部数値を削り、意味ある代表値・境界に絞る |

候補は **132 グループ、298 関数**。これは削除予定数ではない。MERGE でも異なる failure mode の入力は残るため、減った関数数を品質・性能の指標にはしない。

## コストと保証の比較

短い unit の多くは実行時間より fixture・式・定数の二重管理が主な費用となる。固定 LUT / enum 名 / 幅 token の失敗は実害と直結せず、見た目変更に対してノイズとなる。一方、材料モデル・規準境界・sign/axis・キャッシュ更新・データ保全は、誤結果を防ぐ価値が維持費を上回る。

環境準備時の baseline ログでは full_model は通常約71秒、GUI約65秒。今回の個別 benchmark ではない。この長い統合テストは実モデルの配線を保証するため保持する。格子参照同士の case6/8 は本番を呼ばないまま多数の格子を計算し、MCP の重複 smoke は async job / store / 待機を増やすため、削除優先度が高い。個別候補の時間は未計測であり、全体が何秒速くなるとは主張しない。

## 実験で確認した弱い保証

元リポジトリを変更せず `/tmp` の隔離コピーで `ConcreteNewRc` と `Concrete` の `commit` と `revert` を両方 no-op にした。`cargo test -p sepika-material --offline commit_revert` は7件すべて成功し、うち変更した2材料の `test_newrc_commit_revert` / `test_concrete_commit_revert` も成功した。これらは state rollback を保証せず、最後の再 trial の負符号しか検査しない。実験は該当2件の検出力の評価であり、他5材料は変更していない。

## 削らない保証と、Issue の候補から変更した判断

- Cholesky/LU の fresh と値更新後の cache 解の bit 比較、sparse の同じ nnz/count で pattern が変わる guard は保持する。同じ入力を何度も反復する部分だけ削る。Auto fallback の非収束・fallback error と再利用も保持。
- `invert_small` の n=6 は 72 要素固定作業配列の上限を検査する。2次元では上限の stride/境界不具合を見逃すため、6次元入力は保持し A×inverse のループを共有する。
- `test_stress_cfg_default_is_false` を単純削除しない。serde の missing-field テストは Default と自己比較しているので、false の独立 assert を移さないと長期軸力の既定を変えても通る。
- `MemberDetailAttr::is_empty` は編集 roundtrip では呼ばれない。joint-only 属性が UI で削られる不具合を防ぐため、入力→反映の経路へ判定を移してから独立テストを整理する。
- M–θ の「端ばね変形 / 弦からの回転」の定義表示と有効桁数は、任意の固定ラベルとは異なり利用者の判断へ影響する。値/量の抽出とまとめて保証する。
- モックを使う収束失敗/再試行テストは、実際の制御ループを通って rollback・刻み変更・中止を検証するため保持する。参照計算だけを呼ぶ case6/8 と混同しない。
- 規準表、材料/径/厚さの境界、独立手計算、剛性/質量/リリース/符号、履歴・checkpoint、ST-Bridge の外部タグと fixture、OViKA、full_model / wall_model は維持する。単なる derive enum の自己 roundtrip とは区別する。

## 全候補

各候補の「失う検出」は、削る確認の価値と残す/移す保証の両方を示す。ファイル/テストへのリンクは整理前のソース。

### 数学

#### C001 — MERGE

**理由:** 各 backend に同じ未 factorize 契約を別 fixture で検証。

**失う検出・代替保証:** どの backend でも未分解の solve / solve_into が NotFactorized を返すこと。backend ごとの実装があるため対象 backend は削らない。

**具体案:** LinearSolver の共有契約テーブルで Cholesky / LU / PCG / Auto の両 API を実行。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-math/src/auto.rs::test_auto_not_factorized](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-math/src/auto.rs#L258)
- [crates/sepika-math/src/cholesky.rs::test_not_factorized](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-math/src/cholesky.rs#L137)
- [crates/sepika-math/src/lu.rs::test_lu_not_factorized](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-math/src/lu.rs#L102)
- [crates/sepika-math/src/pcg.rs::test_pcg_not_factorized](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-math/src/pcg.rs#L222)

#### C002 — MERGE

**理由:** 同一の RHS 次元不一致契約。

**失う検出・代替保証:** Cholesky / PCG の個別チェック抜け。

**具体案:** 両 backend・solve / solve_into を共有契約テーブルへ。LU / Auto も契約対象とし、正しいエラー variant を確認。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-math/src/cholesky.rs::test_dim_mismatch](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-math/src/cholesky.rs#L144)
- [crates/sepika-math/src/pcg.rs::test_pcg_dim_mismatch](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-math/src/pcg.rs#L229)

#### C003 — SIMPLIFY

**理由:** 通常解と同じ数値を再確認し、空と短い Vec の resize を重ねて検証。

**失う検出・代替保証:** out / scratch の再利用でサイズや値が残る不具合。Rust の Vec::resize 自体を再確認する価値はない。

**具体案:** 両 API の同値性を共有契約へ移し、短い出力バッファの代表例と変更 RHS の再利用だけ残す。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-math/src/cholesky.rs::test_solve_into_matches_solve](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-math/src/cholesky.rs#L319)

#### C004 — SIMPLIFY

**理由:** 非対称行列の解は固有の価値があるが、末尾 solve_into は別 API 契約との重複。

**失う検出・代替保証:** 非対称行列を誤って対称扱いする不具合は残す。

**具体案:** 既知の非対称解を残し、solve_into 同値性を共有契約へ移す。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-math/src/lu.rs::test_lu_unsymmetric](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-math/src/lu.rs#L62)

#### C005 — SIMPLIFY

**理由:** 同じ入力の反復を複数回追加しても別の条件を保証しない。

**失う検出・代替保証:** fresh 同士・値更新後の cached / fresh の bit 不一致。pattern change は別テストで保持。

**具体案:** fresh 2 台の 1 比較と k1→k2 の更新後 1 比較を保持し、同一 factorize / solve の追加ループを削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-math/src/cholesky.rs::test_2dof_deterministic](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-math/src/cholesky.rs#L98)
- [crates/sepika-math/src/cholesky.rs::test_reused_symbolic_matches_fresh_bit_exact](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-math/src/cholesky.rs#L216)

#### C006 — SIMPLIFY

**理由:** 同一 k2 の再 factorize ループが、既存 cached / fresh 比較と重複。

**失う検出・代替保証:** LU の symbolic 再利用で値更新が反映されない不具合。

**具体案:** k1→k2→fresh の bit 比較を 1 回残す。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-math/src/lu.rs::test_reused_symbolic_matches_fresh_bit_exact](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-math/src/lu.rs#L159)

#### C007 — SIMPLIFY

**理由:** factory 検証で Cholesky のばね解を再実行。

**失う検出・代替保証:** make_solver(Auto) が誤った backend を返す配線ミス。

**具体案:** factory の backend 選択だけ残し、10/15 の数値解と solve_into の再検証を共有契約へ。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-math/src/auto.rs::test_make_solver_auto](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-math/src/auto.rs#L185)

#### C008 — SIMPLIFY

**理由:** backend 選択と数値アルゴリズムの検証が混在。

**失う検出・代替保証:** 閾値による選択を逆にする不具合。

**具体案:** 小/大の backend 選択をテーブルへ。PCG/direct の数値精度は test_pcg_agrees_with_direct / test_2dof_spring に任せる。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-math/src/auto.rs::test_auto_small_uses_direct](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-math/src/auto.rs#L168)
- [crates/sepika-math/src/auto.rs::test_auto_large_uses_pcg](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-math/src/auto.rs#L196)

#### C009 — SIMPLIFY

**理由:** Auto は direct solver の決定性保証を繰り返す。

**失う検出・代替保証:** Auto 内部の再 factorize が更新値を渡さない不具合は delegation の範囲を超えないため配線として 1 例保持。

**具体案:** Auto で変更行列を再分解する 1 例だけ共有契約に残し、direct 固有の繰り返し bit 比較を削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-math/src/auto.rs::test_auto_direct_refactorize_matches_fresh_bit_exact](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-math/src/auto.rs#L269)

### Core

#### C010 — DELETE

**理由:** 標準的な dot / cross / norm の逐語的な算術例。要素の局所軸・剛体回転・座標変換テストが結果を直接検証。

**失う検出・代替保証:** 汎用ベクトル演算の誤り。実際の座標変換・剛体運動の数値テストが検出する。

**具体案:** 削除。ZERO_TOL と縮退入力の契約は別テストで保持。

**費用:** 微小。数式の二重記述と低信号の失敗を減らす。

**対象:**

- [crates/sepika-core/src/geom/vec3.rs::dot_cross_norm_の基本則](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/geom/vec3.rs#L79)

#### C011 — MERGE

**理由:** 距離・中点の算術確認と、重要な正規化の縮退契約が混在。

**失う検出・代替保証:** ZERO_TOL の境界や同一点間の方向が NaN になる不具合。

**具体案:** dist / midpoint の例は削除し、unit / unit_from の正常方向と zero・ZERO_TOL・同一点を同じ契約テーブルへ。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/geom/vec3.rs::unit_は縮退ベクトルで_none_を返す](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/geom/vec3.rs#L92)
- [crates/sepika-core/src/geom/vec3.rs::dist_と_midpoint](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/geom/vec3.rs#L99)

#### C012 — SIMPLIFY

**理由:** 変換全関数の単純算術・恒等変換を羅列。

**失う検出・代替保証:** N↔kN、mm↔m、面積の指数、重力の桁間違い。

**具体案:** 代表的な桁変換・面積の指数・逆変換を残す。恒等変換と同じ倍率の重複例は削る。設計重量と物理密度の独立テストは保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/units.rs::test_unit_conversions](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/units.rs#L236)

#### C013 — SIMPLIFY

**理由:** 期待値を同じ GRAVITY 定数で割り直す式と独立数値が二重。

**失う検出・代替保証:** 重力による質量換算の単位や倍率の誤り。

**具体案:** 独立数値 2.4473e-9 との比較を残し、同じ式の再計算を削除。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/units.rs::test_mass_density_from_unit_weight](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/units.rs#L322)

#### C014 — MERGE

**理由:** 対称な役割の誤配置で大きい fixture を複製。

**失う検出・代替保証:** 床に Post / 壁に Beam を許可する各 guard の抜け。

**具体案:** 床/壁の異常配置をテーブル化し、両 branch と期待エラーを保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/model/tests.rs::test_validate_rejects_post_in_floor_region_secondary_beams](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/model/tests.rs#L101)
- [crates/sepika-core/src/model/tests.rs::test_validate_rejects_beam_in_wall_region_posts](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/model/tests.rs#L177)

#### C015 — SIMPLIFY

**理由:** 二次部材に無関係な用途を全 enum 値で展開。

**失う検出・代替保証:** 主材の用途制約を二次部材へ誤適用する不具合。

**具体案:** 未指定と反対用途の代表例を Beam / Post で残す。全 FrameSectionUse の積は削除。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/model/tests.rs::test_validate_allows_secondary_sections_with_any_frame_use](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/model/tests.rs#L210)

#### C016 — MERGE

**理由:** 同じ Material fixture の Some / None の 2 branch。

**失う検出・代替保証:** 明示 G の無視、未指定時の G 導出の誤り。

**具体案:** Some / None テーブルにし、派生値は独立固定値で照合。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/model/tests.rs::test_shear_modulus_explicit](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/model/tests.rs#L241)
- [crates/sepika-core/src/model/tests.rs::test_shear_modulus_derived](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/model/tests.rs#L259)

#### C017 — MERGE

**理由:** Default の単独検査と serde missing-field の検査を分割。後者は本番 Default と比較しており false を独立に固定していない。

**失う検出・代替保証:** no_long_axial の既定が true になると長期軸力を欠落する。両者の単純な片側削除は不可。

**具体案:** missing-field の外部入力テストへ 2 フィールドの literal false assert を移し、単独 Default fixture を削除。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/model/tests.rs::test_stress_cfg_default_is_false](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/model/tests.rs#L520)
- [crates/sepika-core/src/model/tests.rs::test_model_stress_cfg_default_missing_field](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/model/tests.rs#L528)

#### C018 — MERGE

**理由:** new と joints.push の bool だけ。編集 roundtrip は is_empty を呼ばないため代替保証にはならない。

**失う検出・代替保証:** 継手だけの入力を空として UI が消す不具合。現在の joint 非空の検査は移して保持する。

**具体案:** member_details の入力→属性反映テストに空/継手のみの判定を統合。独立 fixture を削り、ハンチの検定位置・量は保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/model/member_detail.rs::test_is_empty](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/model/member_detail.rs#L226)

#### C019 — MERGE

**理由:** 同じ plane fitting 正常契約を方向ごとの fixture に分割。

**失う検出・代替保証:** 各向きで法線が間違う不具合。方向は削らない。

**具体案:** 3 方向の点群と期待法線を table 化。縮退・散乱・collinear は固有契約として保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/geom/tests.rs::fits_vertical_plane](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/geom/tests.rs#L18)
- [crates/sepika-core/src/geom/tests.rs::fits_horizontal_plane](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/geom/tests.rs#L32)
- [crates/sepika-core/src/geom/tests.rs::fits_skewed_vertical_plane](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/geom/tests.rs#L45)

#### C020 — MERGE

**理由:** 同じカテゴリ判定の成功/失敗/case/優先順の分割。

**失う検出・代替保証:** 名称の分類・大文字小文字・判定優先順位の誤り。

**具体案:** 有効/未知/競合する接頭辞のケースを同じ table へ。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/material_grade.rs::test_category_of_grade](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/material_grade.rs#L405)
- [crates/sepika-core/src/material_grade.rs::test_category_of_grade_unknown](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/material_grade.rs#L431)
- [crates/sepika-core/src/material_grade.rs::test_category_of_grade_order](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/material_grade.rs#L440)

#### C021 — MERGE

**理由:** 同じ規準表の厚さ側を別テストに分割。

**失う検出・代替保証:** 40mm 境界・鋼種別 F 値の誤り。規準表の値は削らない。

**具体案:** 鋼種×厚さを 1 table にし、ちょうど 40 と直上を含める。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/material_grade.rs::test_steel_f_value_le40](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/material_grade.rs#L467)
- [crates/sepika-core/src/material_grade.rs::test_steel_f_value_gt40](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/material_grade.rs#L494)

#### C022 — SIMPLIFY

**理由:** presets.len()==14+3+13 が構成リストのコピー。

**失う検出・代替保証:** プリセットの材料定数や grade の誤りは有効な保証。個数自体の変更は不具合ではない。

**具体案:** 固定件数の assert を削除し、代表材料の既知物性を残す。カテゴリ整合テストは別契約として保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/material_grade.rs::test_material_presets](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/material_grade.rs#L735)

#### C023 — MERGE

**理由:** 生成結果の検査が別 fixture として分離。

**失う検出・代替保証:** 新規生成で意図しない断面を自動割当する不具合。

**具体案:** 基本的な生成個数/接続を確認する fixture に section=None の assert を移す。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/frame_gen.rs::test_generated_members_have_no_section](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/frame_gen.rs#L711)

#### C024 — MERGE

**理由:** スラブを生成しない条件で同じ結果を別 setup で確認。

**失う検出・代替保証:** 明示 off と grid 無しの各 branch で余計なスラブを生成する不具合。

**具体案:** 両条件を table にして floor_regions / sections / materials が増えない保証を維持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/frame_gen.rs::test_frame_without_slabs](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/frame_gen.rs#L890)
- [crates/sepika-core/src/frame_gen.rs::test_no_slabs_without_grid_region](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/frame_gen.rs#L977)

#### C122 — MERGE

**理由:** 本番 auto_combinations を expected とする照合は生成規則の誤りを両側で共有する。model/tests の literal terms 検査と id 契約が重複。

**失う検出・代替保証:** 既定ケース id=0/1/3/4 の配線と長短期識別。

**具体案:** model/tests::test_default_combinations の独立 terms fixture に長短期の assert を移して共有比較を削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/load_combo.rs::test_default_combinations_matches_auto_combinations](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/load_combo.rs#L391)

#### C123 — SIMPLIFY

**理由:** 同じ DL/LL を 5 組合せで繰り返し列挙し、case 名/id を default cases test と再確認。

**失う検出・代替保証:** 地震 EX/EY と ±係数・架構用 LL の取り違え。

**具体案:** 独立 literal terms の table は維持し、default_load_cases の名前再確認を一箇所へ集約。新規 Model への wiring は別に残す。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/model/tests.rs::test_default_combinations](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/model/tests.rs#L731)

#### C132 — DELETE

**理由:** serde derive だけの enum を自分で encode/decode して等価比較。独立した wire schema を検査していない。

**失う検出・代替保証:** derive codec の相互一致のみ。rename 等の wire 破壊でも両側が同時に変わるので通る。Basement の構造挙動は階/荷重テストが確認。

**具体案:** 単独 enum roundtrip を削除。プロジェクト保存と schema 入力 fixture の検査は保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-core/src/model/tests.rs::test_story_level_kind_basement_msgpack_roundtrip](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/src/model/tests.rs#L621)

### UI

#### C025 — DELETE

**理由:** enum の default / 固定表示文字列 / LUT に書かれた RGB 値をコピー。

**失う検出・代替保証:** 見た目の既定色・名称・anchor が変更されること。機能不具合と区別できず、仕様変更を止める負担が上回る。

**具体案:** 削除。範囲 clamp・中立白・検定の危険境界は保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/theme.rs::colormap_default_is_viridis](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/theme.rs#L404)
- [crates/sepika-app/src/theme.rs::colormap_sample_matches_endpoint_anchors](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/theme.rs#L410)
- [crates/sepika-app/src/theme.rs::colormap_labels](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/theme.rs#L495)

#### C026 — MERGE

**理由:** 境界検証を同じ色関数の別テストへ分割。

**失う検出・代替保証:** 0.8 / 1.0 の境界と負値 clamp の誤り。

**具体案:** 境界直下/境界/直上と負値を 1 table へ。5.0 の追加赤確認は 1.0001 と重複なので削除。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/theme.rs::check_ratio_color_matches_status_color_at_boundaries](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/theme.rs#L466)
- [crates/sepika-app/src/theme.rs::check_ratio_color_clamps_negative](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/theme.rs#L489)

#### C027 — DELETE

**理由:** theme の LUT と clamp の再検証。緑 channel の単調性は選択 colormap の内部性質。

**失う検出・代替保証:** contour の正規化配線の誤りは選択 map 検証へ移して守る。LUT 自体はここで保証しない。

**具体案:** selected_colormap の代表テストに入力→theme sample への正規化結果を残し、この 3 テストを削除。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/diagram.rs::contour_color_endpoints_and_neutral](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/diagram.rs#L883)
- [crates/sepika-app/src/viewer/diagram.rs::contour_color_clamps_out_of_range](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/diagram.rs#L901)
- [crates/sepika-app/src/viewer/diagram.rs::contour_color_green_channel_is_monotonic](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/diagram.rs#L909)

#### C028 — SIMPLIFY

**理由:** 出力が異なるだけでは正しい正規化か分からない。

**失う検出・代替保証:** 選択 map が無視される / 正規化が誤る不具合。

**具体案:** map.sample(既知の正規化位置) と比較する代表ケースにまとめ、固定 RGB をコピーしない。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/diagram.rs::contour_color_respects_selected_colormap](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/diagram.rs#L920)

#### C029 — DELETE

**理由:** 固定 enum label と draft の Default 内容をそのまま assert。

**失う検出・代替保証:** label / 初期入力の変更を検出するだけ。解析属性の適用・編集結果は実際の editing テストで検証。

**具体案:** 削除。index remap / draft 対象の同一性 / 物性の summary 表示は保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/damper_def_editor.rs::test_damper_kind_label](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/damper_def_editor.rs#L465)
- [crates/sepika-app/src/damper_def_editor.rs::test_damper_def_draft_default_is_maxwell](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/damper_def_editor.rs#L477)

#### C030 — MERGE

**理由:** 同じ summary の物性・モデル branch ごとの文字列検査。

**失う検出・代替保証:** モデルや relief の取り違えで重要な物性が誤表示。

**具体案:** モデルと relief 条件を table 化。固定日本語 label の全文一致は避け、重要物性の表示を保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/damper_def_editor.rs::test_damper_summary_maxwell_without_relief](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/damper_def_editor.rs#L421)
- [crates/sepika-app/src/damper_def_editor.rs::test_damper_summary_maxwell_with_relief](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/damper_def_editor.rs#L436)
- [crates/sepika-app/src/damper_def_editor.rs::test_damper_summary_hysteretic](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/damper_def_editor.rs#L450)

#### C031 — DELETE

**理由:** 内部の幅 token 順、builder の代入、倍率関数の下限を再確認。

**失う検出・代替保証:** 文字の切れは id_column_fits_five_digits / column_widens_for_long_header が具体的に検出。任意の token 順・高さ倍率の変更は実害と対応しない。

**具体案:** 削除。長見出しと ID 可読性、重要桁の format テストを保持。

**費用:** GUI/font 依存の脆さと定数変更時の無意味な修正を減らす。

**対象:**

- [crates/sepika-app/src/table_util.rs::width_tokens_are_ordered](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/table_util.rs#L413)
- [crates/sepika-app/src/table_util.rs::col_keeps_header_and_hover](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/table_util.rs#L441)
- [crates/sepika-app/src/table_util.rs::min_scrolled_height_keeps_several_rows](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/table_util.rs#L454)
- [crates/sepika-app/src/table_util.rs::min_column_width_keeps_three_digits](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/table_util.rs#L463)

#### C032 — DELETE

**理由:** format!("{:.4e}", core定数) の結果だけを確認し、UI コードを呼ばない。

**失う検出・代替保証:** Rust formatter / core 定数の誤り。density の物理値は core・質量・自重テストが検出する。

**具体案:** 削除。

**費用:** ライブラリと定数の再確認であり固有保証なし。

**対象:**

- [crates/sepika-app/src/tables/materials.rs::test_default_custom_density_is_steel_mass_density](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/materials.rs#L460)

#### C033 — DELETE

**理由:** parse_lk_direct に C係数という error label を渡して同じ入力を再実行。C 専用 wrapper 自体は存在しない。

**失う検出・代替保証:** parser の空欄/正値/非正値は test_parse_lk_direct_* で検出。C の実入力 routing はこのテストでも呼んでいない。

**具体案:** C label での同一 parser 再実行を削除。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/tables/steel_attrs.rs::test_parse_c_direct_reuses_parse_lk_direct](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/steel_attrs.rs#L587)

#### C034 — SIMPLIFY

**理由:** 固定の「なし」「自動」文字列と意味ある倍率の表示が混在。

**失う検出・代替保証:** length / percent / C の重要な値を誤表示する不具合。

**具体案:** 固定ラベルの列挙を削り、値・単位・精度がある代表例だけ残す。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/tables/steel_attrs.rs::test_desc_helpers](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/steel_attrs.rs#L597)

#### C035 — MERGE

**理由:** 欠損率 parser の入力ごとに独立した短いテスト。

**失う検出・代替保証:** 欠損率 の受理/拒否/roundtrip の誤り。invalid 条件ごとの guard は捨てない。

**具体案:** 入力と期待結果・error を table 化し、重複 fixture を共有。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/tables/steel_attrs.rs::test_parse_loss_percent_empty_is_zero](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/steel_attrs.rs#L502)
- [crates/sepika-app/src/tables/steel_attrs.rs::test_parse_loss_percent_rejects_non_numeric](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/steel_attrs.rs#L509)

#### C036 — MERGE

**理由:** 横補剛長の 3 欄の部分入力 parser の入力ごとに独立した短いテスト。

**失う検出・代替保証:** 横補剛長の 3 欄の部分入力 の受理/拒否/roundtrip の誤り。invalid 条件ごとの guard は捨てない。

**具体案:** 入力と期待結果・error を table 化し、重複 fixture を共有。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/tables/steel_attrs.rs::test_parse_lb_direct_all_empty_is_none](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/steel_attrs.rs#L516)
- [crates/sepika-app/src/tables/steel_attrs.rs::test_parse_lb_direct_all_filled](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/steel_attrs.rs#L523)
- [crates/sepika-app/src/tables/steel_attrs.rs::test_parse_lb_direct_partial_is_error](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/steel_attrs.rs#L532)
- [crates/sepika-app/src/tables/steel_attrs.rs::test_parse_lb_direct_rejects_non_numeric](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/steel_attrs.rs#L539)

#### C037 — MERGE

**理由:** 整数補剛数 parser の入力ごとに独立した短いテスト。

**失う検出・代替保証:** 整数補剛数 の受理/拒否/roundtrip の誤り。invalid 条件ごとの guard は捨てない。

**具体案:** 入力と期待結果・error を table 化し、重複 fixture を共有。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/tables/steel_attrs.rs::test_parse_brace_count_empty_is_none](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/steel_attrs.rs#L545)
- [crates/sepika-app/src/tables/steel_attrs.rs::test_parse_brace_count_valid](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/steel_attrs.rs#L551)
- [crates/sepika-app/src/tables/steel_attrs.rs::test_parse_brace_count_rejects_non_integer](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/steel_attrs.rs#L558)

#### C038 — MERGE

**理由:** 有効長の正値 parser の入力ごとに独立した短いテスト。

**失う検出・代替保証:** 有効長の正値 の受理/拒否/roundtrip の誤り。invalid 条件ごとの guard は捨てない。

**具体案:** 入力と期待結果・error を table 化し、重複 fixture を共有。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/tables/steel_attrs.rs::test_parse_lk_direct_empty_is_none](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/steel_attrs.rs#L566)
- [crates/sepika-app/src/tables/steel_attrs.rs::test_parse_lk_direct_valid](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/steel_attrs.rs#L572)
- [crates/sepika-app/src/tables/steel_attrs.rs::test_parse_lk_direct_rejects_non_positive_or_invalid](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/steel_attrs.rs#L578)

#### C039 — MERGE

**理由:** 継手位置・種別・区切り parser の入力ごとに独立した短いテスト。

**失う検出・代替保証:** 継手位置・種別・区切り の受理/拒否/roundtrip の誤り。invalid 条件ごとの guard は捨てない。

**具体案:** 入力と期待結果・error を table 化し、重複 fixture を共有。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/tables/member_details.rs::test_parse_joints_empty_is_empty_vec](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/member_details.rs#L462)
- [crates/sepika-app/src/tables/member_details.rs::test_parse_joints_defaults_to_site](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/member_details.rs#L469)
- [crates/sepika-app/src/tables/member_details.rs::test_parse_joints_comma_separated_with_kind](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/member_details.rs#L482)
- [crates/sepika-app/src/tables/member_details.rs::test_parse_joints_newline_separated_english_kind](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/member_details.rs#L501)
- [crates/sepika-app/src/tables/member_details.rs::test_parse_joints_rejects_invalid_distance](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/member_details.rs#L520)
- [crates/sepika-app/src/tables/member_details.rs::test_parse_joints_rejects_invalid_kind](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/member_details.rs#L528)

#### C040 — MERGE

**理由:** 継手の表示と再入力 parser の入力ごとに独立した短いテスト。

**失う検出・代替保証:** 継手の表示と再入力 の受理/拒否/roundtrip の誤り。invalid 条件ごとの guard は捨てない。

**具体案:** 入力と期待結果・error を table 化し、重複 fixture を共有。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/tables/member_details.rs::test_format_joints_roundtrip](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/member_details.rs#L535)
- [crates/sepika-app/src/tables/member_details.rs::test_format_joints_empty](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/member_details.rs#L552)

#### C041 — MERGE

**理由:** ハンチの有効/無効・長さ・省略値 parser の入力ごとに独立した短いテスト。

**失う検出・代替保証:** ハンチの有効/無効・長さ・省略値 の受理/拒否/roundtrip の誤り。invalid 条件ごとの guard は捨てない。

**具体案:** 入力と期待結果・error を table 化し、重複 fixture を共有。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/tables/member_details.rs::test_parse_haunch_disabled_is_none](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/member_details.rs#L558)
- [crates/sepika-app/src/tables/member_details.rs::test_parse_haunch_optional_fields_default_zero](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/member_details.rs#L564)
- [crates/sepika-app/src/tables/member_details.rs::test_parse_haunch_all_fields](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/member_details.rs#L578)
- [crates/sepika-app/src/tables/member_details.rs::test_parse_haunch_rejects_non_positive_or_invalid_length](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/member_details.rs#L592)
- [crates/sepika-app/src/tables/member_details.rs::test_parse_haunch_rejects_non_numeric_optional_fields](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/member_details.rs#L600)

#### C042 — MERGE

**理由:** 開口寸法・offset・区切り parser の入力ごとに独立した短いテスト。

**失う検出・代替保証:** 開口寸法・offset・区切り の受理/拒否/roundtrip の誤り。invalid 条件ごとの guard は捨てない。

**具体案:** 入力と期待結果・error を table 化し、重複 fixture を共有。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/tables/wall_plates.rs::test_parse_openings_empty_is_empty_vec](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/wall_plates.rs#L1754)
- [crates/sepika-app/src/tables/wall_plates.rs::test_parse_openings_comma_separated_with_offset](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/wall_plates.rs#L1762)
- [crates/sepika-app/src/tables/wall_plates.rs::test_parse_openings_newline_separated](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/wall_plates.rs#L1783)
- [crates/sepika-app/src/tables/wall_plates.rs::test_parse_openings_tolerates_whitespace_and_uppercase_x](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/wall_plates.rs#L1804)
- [crates/sepika-app/src/tables/wall_plates.rs::test_parse_openings_rejects_missing_x_separator](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/wall_plates.rs#L1818)
- [crates/sepika-app/src/tables/wall_plates.rs::test_parse_openings_rejects_non_numeric](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/wall_plates.rs#L1825)
- [crates/sepika-app/src/tables/wall_plates.rs::test_parse_openings_rejects_non_positive_dims](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/wall_plates.rs#L1831)
- [crates/sepika-app/src/tables/wall_plates.rs::test_parse_openings_rejects_malformed_offset](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/wall_plates.rs#L1838)

#### C043 — MERGE

**理由:** 開口の表示と再入力 parser の入力ごとに独立した短いテスト。

**失う検出・代替保証:** 開口の表示と再入力 の受理/拒否/roundtrip の誤り。invalid 条件ごとの guard は捨てない。

**具体案:** 入力と期待結果・error を table 化し、重複 fixture を共有。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/tables/wall_plates.rs::test_format_openings_roundtrip](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/wall_plates.rs#L1844)
- [crates/sepika-app/src/tables/wall_plates.rs::test_format_openings_empty](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/wall_plates.rs#L1863)

#### C044 — DELETE

**理由:** Default の getters を並べるのみ。click / begin_edit / 空 grid の lifecycle テストが初期状態から動作を確認。

**失う検出・代替保証:** 初期値だけの変更は外部操作で影響がある範囲で検証。

**具体案:** 削除。必要な非 active 操作は lifecycle テストに残す。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/grid/tests.rs::test_grid_state_starts_inactive](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/grid/tests.rs#L28)

#### C045 — MERGE

**理由:** 同じ TSV の空 cell・CRLF・末尾改行 処理の fixture を分割。

**失う検出・代替保証:** TSV の空 cell・CRLF・末尾改行 の各形状を誤処理する不具合。

**具体案:** 既存ケースを table 化し、違う境界条件の入力は削らない。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/grid/tests.rs::test_parse_tsv_basic](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/grid/tests.rs#L196)
- [crates/sepika-app/src/grid/tests.rs::test_parse_tsv_absorbs_crlf_and_trailing_newlines](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/grid/tests.rs#L204)
- [crates/sepika-app/src/grid/tests.rs::test_parse_tsv_keeps_empty_cells](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/grid/tests.rs#L214)

#### C046 — MERGE

**理由:** 同じ 両軸の反復・非拡張・ragged row 処理の fixture を分割。

**失う検出・代替保証:** 両軸の反復・非拡張・ragged row の各形状を誤処理する不具合。

**具体案:** 既存ケースを table 化し、違う境界条件の入力は削らない。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/grid/tests.rs::test_tile_block_tiles_both_axes](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/grid/tests.rs#L225)
- [crates/sepika-app/src/grid/tests.rs::test_tile_block_returns_original_when_not_expanding](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/grid/tests.rs#L238)
- [crates/sepika-app/src/grid/tests.rs::test_tile_block_ragged_rows_fill_with_empty](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/grid/tests.rs#L254)

#### C047 — MERGE

**理由:** 同じ 矩形/行/列/全選択のコピー 処理の fixture を分割。

**失う検出・代替保証:** 矩形/行/列/全選択のコピー の各形状を誤処理する不具合。

**具体案:** 既存ケースを table 化し、違う境界条件の入力は削らない。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/grid/tests.rs::test_rect_to_tsv](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/grid/tests.rs#L370)
- [crates/sepika-app/src/grid/tests.rs::test_rect_to_tsv_row_col_and_all_selection](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/grid/tests.rs#L378)

#### C048 — SIMPLIFY

**理由:** 単位倍率の単独算術例。core と同じ N→kN と独自 gal / m/s 換算が混在。

**失う検出・代替保証:** gal と速度の表示倍率取り違えは core では検出できない。

**具体案:** N→kN の重複を削り、gal / m/s の換算を実際の応答表示 data 作成テストに統合。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/story_response.rs::test_unit_conversions](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/story_response.rs#L195)

#### C049 — MERGE

**理由:** step の節点列生成で story/floor の fixture を重ねる。

**失う検出・代替保証:** 階と層の index の 1 段ずれ・上下位置の誤り。

**具体案:** story / floor の出力形状と位置を table 化。各解釈の期待座標は維持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/story_response.rs::test_story_step_points_shape](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/story_response.rs#L216)
- [crates/sepika-app/src/story_response.rs::test_floor_points_shape](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/story_response.rs#L225)

#### C050 — SIMPLIFY

**理由:** Cow::Borrowed という内部 allocation 方針を固定。

**失う検出・代替保証:** 表示モデルの内容変更を検出する保証は必要。Cow の variant の変更だけでは表示が壊れない。

**具体案:** Borrowed assert を削り、モデル内容が保持される外部結果だけ確認。clone 性能は測定対象へ。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/mod.rs::no_wall_plates_borrows_without_cloning](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/mod.rs#L1431)

#### C051 — SIMPLIFY

**理由:** Cow の variant を要求する内部最適化テスト。

**失う検出・代替保証:** member 表の内容・id が変わる不具合。

**具体案:** allocation 型の assert を削り、表示対象の内容検証へまとめる。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/tables/members.rs::members_table_view_borrows_without_wall_plates](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/tables/members.rs#L1399)

#### C052 — DELETE

**理由:** 描画定数同士の大小しか検証しない。

**失う検出・代替保証:** 任意の見た目変更を検出するのみ。実際の picking は別テストが保証。

**具体案:** 削除。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/check_ratio.rs::node_marker_radii_keep_ordering](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L1294)

#### C053 — MERGE

**理由:** max / NG / id / missing の同じ reducer に独立 fixture。

**失う検出・代替保証:** 最大値選択、NG 伝播、異なる member の混同、未検定混入。

**具体案:** 複数 id・複数値・OK/NG/None を 1 fixture にして結果の map を照合。空入力の standalone ケースは標準 collect の確認なので削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/check_ratio.rs::max_ratio_by_elem_picks_max_ratio](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L820)
- [crates/sepika-app/src/viewer/check_ratio.rs::max_ratio_by_elem_ng_propagates](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L832)
- [crates/sepika-app/src/viewer/check_ratio.rs::max_ratio_by_elem_all_ok_stays_ok](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L845)
- [crates/sepika-app/src/viewer/check_ratio.rs::max_ratio_by_elem_separates_by_id](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L853)
- [crates/sepika-app/src/viewer/check_ratio.rs::max_ratio_by_elem_empty_input](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L870)
- [crates/sepika-app/src/viewer/check_ratio.rs::max_ratio_by_elem_none_is_excluded](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L878)

#### C054 — MERGE

**理由:** 同じ node reducer の max/NG/id/empty 分割。

**失う検出・代替保証:** 節点別最大値と NG 伝播の誤り。

**具体案:** 複数節点で max / NG を一緒に検証。空 collect の standalone は削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/check_ratio.rs::max_ratio_by_node_picks_max_and_propagates_ng](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L891)
- [crates/sepika-app/src/viewer/check_ratio.rs::max_ratio_by_node_separates_by_id](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L900)
- [crates/sepika-app/src/viewer/check_ratio.rs::max_ratio_by_node_empty_input](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L911)

#### C055 — MERGE

**理由:** 同じ filter 契約の branch を細かく分割。

**失う検出・代替保証:** kind filter、同一 kind 最大値、absent/skipped の誤り。

**具体案:** filter 種別と期待 Option を table 化し、実際の各 branch を保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/check_ratio.rs::ratio_for_filter_max_returns_ratio_and_ok](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L920)
- [crates/sepika-app/src/viewer/check_ratio.rs::ratio_for_filter_kind_picks_matching_component](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L937)
- [crates/sepika-app/src/viewer/check_ratio.rs::ratio_for_filter_kind_absent_returns_none](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L965)
- [crates/sepika-app/src/viewer/check_ratio.rs::ratio_for_filter_kind_multiple_same_kind_picks_max](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L982)
- [crates/sepika-app/src/viewer/check_ratio.rs::ratio_for_filter_skipped_returns_none](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L1006)

#### C056 — MERGE

**理由:** present/order と absent/empty が同じ列挙処理。

**失う検出・代替保証:** 未存在項目を出す / 表示順が不定になる不具合。

**具体案:** presence 混在の 1 fixture で項目と定義順を検証。空入力の独立テストを削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/check_ratio.rs::available_check_kinds_returns_present_kinds_in_definition_order](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L1043)
- [crates/sepika-app/src/viewer/check_ratio.rs::available_check_kinds_excludes_absent_kinds](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L1063)
- [crates/sepika-app/src/viewer/check_ratio.rs::available_check_kinds_empty_input](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L1075)

#### C057 — MERGE

**理由:** 閾値と全表示 flag のケース分割。

**失う検出・代替保証:** 閾値の等号や全表示 override の誤り。

**具体案:** 境界/直下と show_all を table 化し、同じ branch の余分な比率例は削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/check_ratio.rs::should_label_only_at_or_above_threshold_by_default](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L1084)
- [crates/sepika-app/src/viewer/check_ratio.rs::should_label_all_shows_every_ratio](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L1094)

#### C058 — MERGE

**理由:** dominant の Some/None と小数整形を別 fixture。

**失う検出・代替保証:** 優勢項目の表示、比率の丸めの誤り。

**具体案:** dominant optional / rounding の代表例に集約し、固定 label そのものの検証は減らす。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/check_ratio.rs::mid_label_text_with_dominant](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L1104)
- [crates/sepika-app/src/viewer/check_ratio.rs::mid_label_text_without_dominant](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L1110)

#### C059 — MERGE

**理由:** 同じ id filter を成功/不在で分離。

**失う検出・代替保証:** 別 member の検定位置が混ざる不具合。

**具体案:** 存在/不在 id を 1 fixture で検証。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/check_ratio.rs::elem_check_positions_filters_by_elem_id](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L1118)
- [crates/sepika-app/src/viewer/check_ratio.rs::elem_check_positions_unknown_elem_returns_empty](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/check_ratio.rs#L1151)

#### C060 — MERGE

**理由:** 同じ aggregation の dedup・level・ductility・step・id/end 分割。

**失う検出・代替保証:** id/end を混同する、最大 level/ductility・最初の step を失う不具合。

**具体案:** 複数 id/end と重複 records を 1 fixture にし、集約後全フィールドを検証。空 collect の単独ケースは削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/hinge.rs::aggregate_hinges_dedups_same_elem_and_end](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1387)
- [crates/sepika-app/src/viewer/hinge.rs::aggregate_hinges_picks_highest_level_and_max_ductility](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1399)
- [crates/sepika-app/src/viewer/hinge.rs::aggregate_hinges_keeps_min_step_as_first_step](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1413)
- [crates/sepika-app/src/viewer/hinge.rs::aggregate_hinges_separates_by_end](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1426)
- [crates/sepika-app/src/viewer/hinge.rs::aggregate_hinges_separates_by_elem](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1443)
- [crates/sepika-app/src/viewer/hinge.rs::aggregate_hinges_empty_input](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1456)

#### C061 — MERGE

**理由:** 同じ大小/同値の軸選択を別テスト化。

**失う検出・代替保証:** 弱軸選択や tie 強軸の取り違え。

**具体案:** strong>weak / weak>strong / tie を table 化。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/hinge.rs::dominant_bend_axis_z_picks_larger_axis](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1481)
- [crates/sepika-app/src/viewer/hinge.rs::dominant_bend_axis_z_picks_weak_axis_when_larger](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1488)
- [crates/sepika-app/src/viewer/hinge.rs::dominant_bend_axis_z_ties_favor_strong_axis](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1495)

#### C062 — MERGE

**理由:** 同じ category 判定の 3 結果。

**失う検出・代替保証:** 弾性・引張降伏・圧縮降伏の符号取り違え。

**具体案:** 3 結果を table 化し、符号条件を保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/hinge.rs::fiber_category_elastic](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1728)
- [crates/sepika-app/src/viewer/hinge.rs::fiber_category_tension_yield](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1734)
- [crates/sepika-app/src/viewer/hinge.rs::fiber_category_compression_yield](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1740)

#### C063 — MERGE

**理由:** 同じ断面選択の i/j/empty 分割。

**失う検出・代替保証:** xi の最小/最大と空入力の取り違え。

**具体案:** 1 断面 fixture に i/j と不在の期待 Option を table 化。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/hinge.rs::pick_fiber_section_i_end_picks_min_xi](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1692)
- [crates/sepika-app/src/viewer/hinge.rs::pick_fiber_section_j_end_picks_max_xi](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1700)
- [crates/sepika-app/src/viewer/hinge.rs::pick_fiber_section_empty_input](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1708)

#### C064 — MERGE

**理由:** MN 表示の軸選択で同じ data fixture を分割。

**失う検出・代替保証:** 軸力の符号反転と強弱軸を混同する不具合。

**具体案:** strong / weak の独立期待値を持つ table へ。out-of-range は別 guard として保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/hinge.rs::mn_beta_columns_weak_axis](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1638)
- [crates/sepika-app/src/viewer/hinge.rs::mn_beta_columns_strong_axis](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1644)
- [crates/sepika-app/src/viewer/hinge.rs::extract_mn_meridian_weak_axis_flips_n_sign](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1652)
- [crates/sepika-app/src/viewer/hinge.rs::extract_mn_meridian_strong_axis_uses_mz](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1669)

#### C065 — MERGE

**理由:** モデルによる端ばね変形/弦からの回転の定義を label と series で別々に検査。

**失う検出・代替保証:** 物理量が異なるのに誤った定義を表示して利用者が誤解する。単なる任意 label ではない。

**具体案:** concentrated/fiber の series 抽出契約に表示定義の検査を統合。文言全文一致は要求せず、量の区別を保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/hinge.rs::m_theta_axis_label_matches_axis](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1547)

#### C066 — MERGE

**理由:** core centroid の矩形計算を表示 wrapper で反復。

**失う検出・代替保証:** 表示 wrapper が offset を失う配線ミス。

**具体案:** offset 矩形の 1 例だけを保持し、原点正方形を削除。非対称 channel と fibers 整合の検証は保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/hinge.rs::section_outline_centroid_of_centered_square_is_origin](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1752)
- [crates/sepika-app/src/viewer/hinge.rs::section_outline_centroid_of_offset_rect_matches_geometric_center](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/hinge.rs#L1761)

#### C067 — SIMPLIFY

**理由:** 座標数=10・振幅全点など内部分割方法への結合が強い。

**失う検出・代替保証:** 端点違い・縮退で非有限になる・ばね記号の折返しが消える不具合。

**具体案:** 端点・有限性・代表的折返しを 1 test へ。固定の点数と全点の振幅一致を削り、zero_coils / 同一点を table 化。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/support_symbols.rs::zigzag_points_endpoints_match_from_to](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/support_symbols.rs#L369)
- [crates/sepika-app/src/viewer/support_symbols.rs::zigzag_points_alternates_perpendicular_side](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/support_symbols.rs#L380)
- [crates/sepika-app/src/viewer/support_symbols.rs::zigzag_points_zero_coils_or_degenerate_returns_two_points](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/support_symbols.rs#L399)

#### C068 — SIMPLIFY

**理由:** 全 segment の線形半径と角度の数値を実装式どおり確認。

**失う検出・代替保証:** 有限な渦巻きでなくなる / 空分割が例外になる不具合。

**具体案:** segment 数・全点の式は削り、0 と代表 segment の有限値・端点・半径変化だけに絞る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/support_symbols.rs::spiral_fracs_monotonic_and_bounded](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/support_symbols.rs#L410)
- [crates/sepika-app/src/viewer/support_symbols.rs::spiral_fracs_zero_segments_is_empty](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/support_symbols.rs#L424)

#### C069 — SIMPLIFY

**理由:** 固定の板位置と layer line の件数を再確認。

**失う検出・代替保証:** 中心から片側へずれる、ゼロ layer で不正になる不具合。

**具体案:** 対称性・有限性と zero layer の結果を 1 table へ。具体的 ±9 と line count のコピーを削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/support_symbols.rs::isolator_marker_geometry_is_symmetric_about_center](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/support_symbols.rs#L431)
- [crates/sepika-app/src/viewer/support_symbols.rs::isolator_marker_geometry_zero_layers_has_no_layer_lines](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/support_symbols.rs#L453)

#### C070 — MERGE

**理由:** 同じ supported isolator の i/j 向きを別 fixture。

**失う検出・代替保証:** 端部順によって target が変わる不具合。

**具体案:** normal / reverse を 1 table 化。both fixed/free と nonzero length は固有 filtering 契約として保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/support_symbols.rs::support_isolators_finds_target_when_other_side_fixed](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/support_symbols.rs#L509)
- [crates/sepika-app/src/viewer/support_symbols.rs::support_isolators_order_independent](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/support_symbols.rs#L519)

#### C124 — MERGE

**理由:** 同じ support visibility の二つの bool を別テストで確認。

**失う検出・代替保証:** lumped view で余計な拘束を描く / frame view の toggle を無視する不具合。

**具体案:** 2×2 の truth table に共有。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/support.rs::supports_hidden_in_lumped_view_even_when_toggle_on](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/support.rs#L297)
- [crates/sepika-app/src/viewer/support.rs::supports_follow_toggle_in_frame_view](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/support.rs#L303)

#### C125 — MERGE

**理由:** draws_as_line は element_draw_shape の Line 判定で、全 ElementKind の mapping を重複列挙。

**失う検出・代替保証:** 面要素/PanelZone が誤った線や highlight として描かれる不具合。

**具体案:** element_draw_shape の独立期待形状を table にし、draws_as_line の delegating assert は一代表だけに絞る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/scene.rs::仕口パネルと面要素は部材線として描かない](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/scene.rs#L932)
- [crates/sepika-app/src/viewer/scene.rs::要素の描き方は種別ごとに一意に決まる](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/scene.rs#L953)

#### C126 — SIMPLIFY

**理由:** 7 / 4 という見た目定数と sqrt の式の再展開を照合。

**失う検出・代替保証:** mass が大きいのに marker が小さい、zero 最大 mass で NaN、最低可読サイズ消失。

**具体案:** 有限性・単調性・zero fallback へ絞り、固定半径と同じ sqrt 式のコピーを削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/lumped.rs::mass_marker_radius_scales_with_sqrt_mass](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/lumped.rs#L336)

#### C127 — SIMPLIFY

**理由:** 初期 zoom=3 と内部 scale=0.32 の定数をコピー。

**失う検出・代替保証:** 細長い viewport で MN 図がはみ出す / zoom 操作が効かない不具合。

**具体案:** 縦横 viewport で収まることと zoom の実際の座標変化に置き換え、任意 default zoom 数値の固定を削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-app/src/viewer/mn_draw.rs::view_scaleは短辺基準で既定ズーム3_0のとき短辺の0_32倍になる](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/src/viewer/mn_draw.rs#L190)

### 要素

#### C071 — MERGE

**理由:** A×inverse=I の同じ検査ループを 2 次元と 6 次元で複写。

**失う検出・代替保証:** 6 次元専用の 72 要素固定作業配列の境界・stride 不具合は 2 次元では検出できない。n=6 の入力自体は削除不可。

**具体案:** n=2 / n=6 を同じ積検証 helper/table に統合。Issue の n=6 全削除提案は採らない。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-element/src/linalg.rs::test_invert_small_identity_roundtrip](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-element/src/linalg.rs#L70)
- [crates/sepika-element/src/linalg.rs::test_invert_small_max_size_six](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-element/src/linalg.rs#L119)

#### C072 — SIMPLIFY

**理由:** prefix を同じ node_global_dofs helper で計算して照合。

**失う検出・代替保証:** append が既存 DOF を上書きする不具合と panel restraint の誤り。

**具体案:** 独立した期待 DOF 列で順序と prefix 保持を node_global_dofs の検査へ統合し、同じ helper からの期待値作成を削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-element/src/behavior.rs::push_helpers_append_to_existing_sequence](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-element/src/behavior.rs#L611)

#### C073 — MERGE

**理由:** 同一剛体回転契約の member 向きごとの分割。

**失う検出・代替保証:** 局所軸の向き別に剛体回転へ偽の剛性が発生する不具合。

**具体案:** 全向きを table 化し、独立な期待ゼロ内力を維持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-element/src/wall/wall_element.rs::test_wall_element_rigid_rotation_horizontal_axis_zero_force](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-element/src/wall/wall_element.rs#L1698)
- [crates/sepika-element/src/wall/wall_element.rs::test_wall_element_rigid_rotation_normal_axis_zero_force](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-element/src/wall/wall_element.rs#L1703)
- [crates/sepika-element/src/wall/wall_element.rs::test_wall_element_rigid_rotation_vertical_axis_zero_force](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-element/src/wall/wall_element.rs#L1708)

#### C074 — MERGE

**理由:** capacity の input guard ごとの fixture 重複。

**失う検出・代替保証:** 材料/配筋の欠落・負値の guard が抜ける不具合。

**具体案:** 共通正常 fixture の 1 項目だけを変える table にし、個別エラー理由を保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-element/src/wall/wall_element.rs::test_issue_when_fc_unset](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-element/src/wall/wall_element.rs#L2638)
- [crates/sepika-element/src/wall/wall_element.rs::test_issue_when_fc_not_positive](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-element/src/wall/wall_element.rs#L2648)
- [crates/sepika-element/src/wall/wall_element.rs::test_issue_when_material_missing](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-element/src/wall/wall_element.rs#L2658)
- [crates/sepika-element/src/wall/wall_element.rs::test_issue_when_section_missing](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-element/src/wall/wall_element.rs#L2669)

#### C128 — MERGE

**理由:** 同じ two-node fixture で forward の mapping と reversed を別検査。

**失う検出・代替保証:** 入力 node 順の無視・restrained DOF 番兵の誤り。

**具体案:** normal/reverse を table 化し、独立した番号/番兵の期待値を持たせる。自分の出力の入替えを expected とする比較を削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-element/src/behavior.rs::node_global_dofs_orders_by_node_and_marks_restrained](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-element/src/behavior.rs#L582)
- [crates/sepika-element/src/behavior.rs::node_global_dofs_follows_given_order](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-element/src/behavior.rs#L598)

### 材料

#### C075 — DELETE

**理由:** revert 後に再 trial し stress<0 のみ。revert を無効にしても同じ envelope で負値となる。

**失う検出・代替保証:** commit / revert の状態破壊を検出していない。圧縮応力符号は既知応力/peak テストが検出。

**具体案:** 削除。状態保証は probe non-mutation と履歴を持つ材料の snapshot / revert 検証で行う。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-material/src/newrc.rs::test_newrc_commit_revert](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-material/src/newrc.rs#L335)

#### C076 — DELETE

**理由:** revert の前後を比較せず、再 trial の符号しか確認しない。

**失う検出・代替保証:** revert を壊しても失敗しない。圧縮 envelope の符号は compression_peak 等で検証済み。

**具体案:** 削除。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-material/src/uniaxial/concrete.rs::test_concrete_commit_revert](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-material/src/uniaxial/concrete.rs#L277)

#### C077 — DELETE

**理由:** 反転の形状を assert せず peak>fy のみ。単調 hardening でも通る。

**失う検出・代替保証:** Bauschinger 効果の消失を検出できない。hardening / shifted asymptote / R degradation は別の数値検査が検出。

**具体案:** 削除。test_reversal_targets_shifted_asymptote_intersection / test_r_degrades_after_plastic_excursion / snapshots を保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-material/src/uniaxial/menegotto_pinto.rs::test_menegotto_pinto_bauschinger_loop](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-material/src/uniaxial/menegotto_pinto.rs#L332)

#### C078 — DELETE

**理由:** 広い範囲 0.0015<eps<0.003 のみ。既知の envelope 応力点がより強くピーク位置を検証。

**失う検出・代替保証:** ピークひずみの誤りは test_newrc_refactor_matches_known_values の独立固定値と peak テストが検出。

**具体案:** 削除。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-material/src/newrc.rs::test_newrc_eps_c0_reasonable](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-material/src/newrc.rs#L297)

#### C079 — MERGE

**理由:** 同じ origin/初期 tangent の direct / wrapper / tiny strain 比較が重なる。

**失う検出・代替保証:** 厳密 0 と極小正/負で初期 tangent を誤る不具合。

**具体案:** origin と微小 strain の table に direct envelope / wrapper を集約。厳密 0 の branch は削らない。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-material/src/newrc.rs::test_newrc_tangent_at_exactly_zero_strain_is_ec](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-material/src/newrc.rs#L273)
- [crates/sepika-material/src/newrc.rs::test_newrc_envelope_compression_at_origin](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-material/src/newrc.rs#L289)
- [crates/sepika-material/src/newrc.rs::test_newrc_envelope_initial_tangent_is_ec](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-material/src/newrc.rs#L363)

#### C080 — MERGE

**理由:** 明示 Ec=18000 を wrapper と envelope で別 setup。

**失う検出・代替保証:** override Ec が wrapper→envelope に伝わらない不具合。

**具体案:** explicit Ec の 1 fixture で wrapper と envelope の両契約を比較し、重複係数組立てを削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-material/src/newrc.rs::test_newrc_explicit_initial_tangent_changes_envelope_coefficients](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-material/src/newrc.rs#L373)
- [crates/sepika-material/src/newrc.rs::test_newrc_explicit_initial_tangent_applies_to_compression_envelope](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-material/src/newrc.rs#L381)

### 設計

#### C081 — DELETE

**理由:** pub use の再 export を runtime 数値で再確認。

**失う検出・代替保証:** re-export が失われれば利用箇所がコンパイル不成立。F 規準表は core の boundary テストが検証する。

**具体案:** 削除。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-design-jp/src/material_strength/mod.rs::test_steel_f_value_reexport_wires_to_core](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/material_strength/mod.rs#L152)

#### C082 — DELETE

**理由:** 同じ fc24 の長期 8 / 短期16 がより厳しい tolerance の代表値テストに存在。

**失う検出・代替保証:** test_concrete_fc24_representative_values が同じ入力・出力を検出。

**具体案:** 削除。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-design-jp/src/material_strength/mod.rs::test_concrete_compression_short_is_2x_long](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/material_strength/mod.rs#L41)

#### C083 — SIMPLIFY

**理由:** lightweight の同じ倍率を fc24 固定値テストと重複し、normal 圧縮値も再確認。

**失う検出・代替保証:** Lightweight1/2 の各 branch 誤りは fc24 の 0.666 / 0.999 で検出。

**具体案:** 重複例を削る。異なる入力を使うなら規準 branch 境界に意味がある値へ統合する。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-design-jp/src/material_strength/mod.rs::test_lightweight_concrete_is_0_9x](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/material_strength/mod.rs#L48)

#### C084 — SIMPLIFY

**理由:** 本番 big_lambda を expected に使うか同じ式を再展開。big_lambda のミスは共有 expected に伝播する。

**失う検出・代替保証:** 細長比の倍率/elastic 圧縮強度の誤りを独立に検出する価値は必要。

**具体案:** 独立固定値 Lambda=119.7891 と lambda=300 の許容応力度で照合。式の二重計算・expected 自体の assert を削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-design-jp/src/material_strength/mod.rs::test_big_lambda_representative_value](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/material_strength/mod.rs#L188)
- [crates/sepika-design-jp/src/material_strength/mod.rs::test_steel_fc_elastic_branch_matches_formula](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/material_strength/mod.rs#L212)

#### C085 — DELETE

**理由:** p.at>0 && p.pw>0 の smoke は同じ円形 shape の詳細 props 比較より弱い。

**失う検出・代替保証:** test_axis_props_from_shape_circle_matches_props と ultimate の circle hoop / independent values が検出。

**具体案:** 削除。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-design-jp/src/rc/section_props.rs::test_axis_props_from_shape_circle](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/rc/section_props.rs#L330)

#### C086 — SIMPLIFY

**理由:** 本番 ratio から耐力を逆算して同じ本番へ 0.3 倍を戻す。my=0 で biaxial sum も実際には検証しない。

**失う検出・代替保証:** 耐力の一定倍率の誤り・弱軸加算の欠落を見逃す。

**具体案:** 独立に算定した強弱両軸の需要/耐力の代表検査へ統合。単軸 0.3 倍の自己照合は削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-design-jp/src/rc/column/mod.rs::test_column_biaxial_linear_sum](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/rc/column/mod.rs#L724)

#### C087 — SIMPLIFY

**理由:** 本番結果を逆数にして 0.3 倍を再投入する自己照合。

**失う検出・代替保証:** 両軸加算や耐力の倍率誤りを検出できない。

**具体案:** SRC 固有の両軸独立値テストへ統合し、単軸比例性の自己照合を削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-design-jp/src/srrc/column.rs::test_src_column_biaxial_linear_sum](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/srrc/column.rs#L508)

#### C088 — MERGE

**理由:** 同じ C 解決関数の曲率/override/fallback ごとに短いテスト。

**失う検出・代替保証:** C の cap・曲率符号・direct 優先・partial Lb の各取り違え。

**具体案:** 各 branch の入力/期待 C を table 化。規準値と boundary は削らない。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-design-jp/src/steel/section.rs::test_c_factor_double_curvature_equal_clamps_to_2_3](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/section.rs#L551)
- [crates/sepika-design-jp/src/steel/section.rs::test_c_factor_single_curvature_uniform_is_1_0](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/section.rs#L563)
- [crates/sepika-design-jp/src/steel/section.rs::test_c_factor_m2_zero_is_1_75](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/section.rs#L574)
- [crates/sepika-design-jp/src/steel/section.rs::test_c_factor_mid_moment_dominant_is_1_0](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/section.rs#L585)
- [crates/sepika-design-jp/src/steel/section.rs::test_c_factor_none_end_moments_is_1_0](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/section.rs#L597)
- [crates/sepika-design-jp/src/steel/section.rs::test_c_factor_diff_sign_gt_same_sign_for_same_ratio](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/section.rs#L611)
- [crates/sepika-design-jp/src/steel/section.rs::test_c_factor_direct_input_overrides_auto](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/section.rs#L639)
- [crates/sepika-design-jp/src/steel/section.rs::test_c_factor_partial_lb_without_direct_is_1_0](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/section.rs#L667)
- [crates/sepika-design-jp/src/steel/section.rs::test_c_factor_direct_non_positive_falls_back](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/section.rs#L681)

#### C089 — MERGE

**理由:** 同じ Lb 優先順位の direct/brace/length 分割。

**失う検出・代替保証:** direct 指定と補剛数の優先順位・fallback の誤り。

**具体案:** 優先順位 table へ。全 branch 保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-design-jp/src/steel/section.rs::test_resolve_lb_direct_input_priority](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/section.rs#L823)
- [crates/sepika-design-jp/src/steel/section.rs::test_resolve_lb_brace_count_when_no_direct](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/section.rs#L836)
- [crates/sepika-design-jp/src/steel/section.rs::test_resolve_lb_falls_back_to_length](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/section.rs#L844)

#### C090 — MERGE

**理由:** 同じ lambda parameter の branch ごとの fixture。

**失う検出・代替保証:** 曲率・partial Lb・unknown moment の安全側選択を誤る不具合。

**具体案:** 独立期待 0.3 / 0.9 の table 化。partial/full branch は維持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-design-jp/src/steel/beam.rs::test_p_lambda_b_mid_moment_dominant_is_0_3](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/beam.rs#L1334)
- [crates/sepika-design-jp/src/steel/beam.rs::test_p_lambda_b_single_curvature_uniform_is_0_3](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/beam.rs#L1346)
- [crates/sepika-design-jp/src/steel/beam.rs::test_p_lambda_b_double_curvature_uniform_is_0_9](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/beam.rs#L1357)
- [crates/sepika-design-jp/src/steel/beam.rs::test_p_lambda_b_none_end_moments_is_0_3](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/beam.rs#L1368)
- [crates/sepika-design-jp/src/steel/beam.rs::test_p_lambda_b_partial_lb_is_0_3_independent_of_end_moments](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/steel/beam.rs#L1379)

#### C091 — MERGE

**理由:** Fs / Fe 境界とその組合せで setup と同じ値を反復。

**失う検出・代替保証:** 0.6 / 0.15 の境界と Fes の積・下限を誤る不具合。

**具体案:** 係数と組合せを boundary table にまとめ、独立した規準期待値を保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-design-jp/src/secondary/holding_capacity.rs::test_fs_ge_06](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/secondary/holding_capacity.rs#L242)
- [crates/sepika-design-jp/src/secondary/holding_capacity.rs::test_fs_lt_06](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/secondary/holding_capacity.rs#L248)
- [crates/sepika-design-jp/src/secondary/holding_capacity.rs::test_fe_le_015](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/secondary/holding_capacity.rs#L254)
- [crates/sepika-design-jp/src/secondary/holding_capacity.rs::test_fe_gt_015](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/secondary/holding_capacity.rs#L260)
- [crates/sepika-design-jp/src/secondary/holding_capacity.rs::test_fes_default](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/secondary/holding_capacity.rs#L266)
- [crates/sepika-design-jp/src/secondary/holding_capacity.rs::test_fes_example](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/secondary/holding_capacity.rs#L272)

#### C092 — MERGE

**理由:** 同じ rc_bar_props 矩形柱の強弱軸に旧/新 fixture の重複。

**失う検出・代替保証:** 幅/せいの入替え・有効せい・shear legs の取り違え。

**具体案:** 強弱軸を 1 table にし、違う配筋値が branch を検査する場合のみ入力を保持。同じ shape/axis の duplicate assert は削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-design-jp/src/ultimate/rc_props.rs::test_rc_rect_strong_props](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/ultimate/rc_props.rs#L315)
- [crates/sepika-design-jp/src/ultimate/rc_props.rs::test_rc_rect_weak_props](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/ultimate/rc_props.rs#L335)
- [crates/sepika-design-jp/src/ultimate/rc_props.rs::test_rc_column_rect_strong_props](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/ultimate/rc_props.rs#L385)
- [crates/sepika-design-jp/src/ultimate/rc_props.rs::test_rc_column_rect_weak_props](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/ultimate/rc_props.rs#L401)

#### C093 — MERGE

**理由:** 同じ梁配筋の top / bottom branch を別 setup。

**失う検出・代替保証:** 非対称配筋の引張側取り違え。

**具体案:** 非対称 shape の両側を table 化。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-design-jp/src/ultimate/rc_props.rs::test_rc_beam_rect_top_tension](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/ultimate/rc_props.rs#L357)
- [crates/sepika-design-jp/src/ultimate/rc_props.rs::test_rc_beam_rect_bottom_tension](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/ultimate/rc_props.rs#L369)

#### C094 — MERGE

**理由:** 同一 CFT 入力の細長比領域を別 fixture。

**失う検出・代替保証:** medium/long の領域判定・耐力減少の誤り。

**具体案:** short/medium/long の table に期待 class と大小をまとめ、各領域は維持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-design-jp/src/ultimate/cft.rs::test_cft_long_column_less_than_short](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/ultimate/cft.rs#L280)
- [crates/sepika-design-jp/src/ultimate/cft.rs::test_cft_medium_column_between_short_and_long](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/ultimate/cft.rs#L295)

#### C095 — SIMPLIFY

**理由:** 円形 Mu>0 だけでは形状 branch の誤係数を検出しない。

**失う検出・代替保証:** 円形 CFT の moment capacity 消失のみ。

**具体案:** 円形の curve/endpoints/独立数値へ統合し、単独正値 smoke を削る。角形の検査だけに代替させない。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-design-jp/src/ultimate/cft_nm.rs::test_cft_short_column_mu_circular_positive](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-design-jp/src/ultimate/cft_nm.rs#L332)

### 編集

#### C096 — MERGE

**理由:** 同じ undo-stack 契約を command ごとの fixture で分割。

**失う検出・代替保証:** 各 command の invalid-id guard の抜け。別 apply 実装なので command 自体は削らない。

**具体案:** command factories の共有 invalid-id 契約 table にし、model unchanged / undo 不追加を全 command で確認。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-edit/src/tests.rs::test_set_node_coord_invalid_id_is_noop](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-edit/src/tests.rs#L123)
- [crates/sepika-edit/src/tests.rs::test_set_node_restraint_invalid_id_is_noop](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-edit/src/tests.rs#L168)
- [crates/sepika-edit/src/tests.rs::test_set_node_support_spring_invalid_id_is_noop](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-edit/src/tests.rs#L252)
- [crates/sepika-edit/src/tests.rs::test_edit_section_shape_invalid_id_noop](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-edit/src/tests.rs#L970)
- [crates/sepika-edit/src/tests.rs::test_set_section_material_on_missing_section_is_noop](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-edit/src/tests.rs#L1440)
- [crates/sepika-edit/src/tests.rs::test_set_story_weight_invalid_id_is_noop](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-edit/src/tests.rs#L2264)
- [crates/sepika-edit/src/tests.rs::test_set_load_case_kind_invalid_id_is_noop](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-edit/src/tests.rs#L2494)
- [crates/sepika-edit/src/tests.rs::test_set_wall_plate_attrs_missing_id_is_noop](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-edit/src/tests.rs#L2915)

#### C097 — MERGE

**理由:** 側テーブル remove の同じ absent / undo 契約。

**失う検出・代替保証:** どちらかの remove が不在時に undo を積む不具合。

**具体案:** 共有 absent 属性 table にまとめ、両 apply を保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-edit/src/tests.rs::test_remove_member_detail_attr_missing_is_noop](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-edit/src/tests.rs#L3728)
- [crates/sepika-edit/src/tests.rs::test_remove_steel_design_attr_missing_is_noop](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-edit/src/tests.rs#L3807)

#### C098 — MERGE

**理由:** 二次部材 section の有効/無効の同一 fixture。

**失う検出・代替保証:** CFT を Beam/Post に許す不具合、正常 steel を拒否する不具合。

**具体案:** Beam/Post×steel/CFT の table にまとめ、各実装 branch を保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-edit/src/tests.rs::add_unassigned_beam_rejects_cft_section](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-edit/src/tests.rs#L6389)
- [crates/sepika-edit/src/tests.rs::add_unassigned_post_rejects_cft_section](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-edit/src/tests.rs#L6402)
- [crates/sepika-edit/src/tests.rs::add_unassigned_beam_accepts_steel_section](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-edit/src/tests.rs#L6415)

### 荷重

#### C099 — DELETE

**理由:** segment_extension / supporting_line というテスト専用参照関数だけを呼ぶ。case6 は split の保存・対称性、case8 は参照モデル同士の差を測る。

**失う検出・代替保証:** 本番 polygon_edge_areas を壊しても失敗しない。参照モデルの性質を失うが本番回帰保証の損失はない。

**具体案:** 通常テストから削除。比較研究として残すなら手動検証資料/計測へ移す。本番との独立比較 case7/9/10/11 は保持。

**費用:** 複数形状の細密格子計算と大量出力を通常 test から除ける。ケース単独秒数は未計測。

**対象:**

- [crates/sepika-load/src/floor/distribution_verification.rs::case6_equal_distance_split_vs_unsplit](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-load/src/floor/distribution_verification.rs#L520)
- [crates/sepika-load/src/floor/distribution_verification.rs::case8_distance_interpretation_sensitivity](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-load/src/floor/distribution_verification.rs#L1228)

#### C100 — MERGE

**理由:** 同じ mass-variant 契約に材料別 fixture。

**失う検出・代替保証:** LumpedOnly / CorrectedLumped の物理総質量が材料ごとに異なる不具合。

**具体案:** 構造種・付帯質量の期待物理値を table 化。steel/RC/CFT/slab 等の異なる source は削らない。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-load/src/story_gen/tests.rs::test_both_mass_methods_have_equal_total_dynamic_mass](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-load/src/story_gen/tests.rs#L1018)
- [crates/sepika-load/src/story_gen/tests.rs::test_both_mass_methods_equal_for_rc_beam_with_slab_deduction](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-load/src/story_gen/tests.rs#L1136)
- [crates/sepika-load/src/story_gen/tests.rs::test_both_mass_methods_equal_with_secondary_member_and_damper](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-load/src/story_gen/tests.rs#L1143)
- [crates/sepika-load/src/story_gen/tests.rs::test_both_mass_methods_equal_with_steel_weight_factor](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-load/src/story_gen/tests.rs#L1214)
- [crates/sepika-load/src/story_gen/tests.rs::test_both_mass_methods_equal_for_cft_column](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-load/src/story_gen/tests.rs#L1303)

#### C101 — MERGE

**理由:** 同じ beam-cover の範囲/順序/非該当契約。

**失う検出・代替保証:** 非共線梁への誤伝達・分割順と全長の欠落。

**具体案:** 1 座標 fixture の照会条件を table 化。順序・範囲の各期待値を保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-load/src/secondary.rs::test_covers_subdivided_beams_in_order](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-load/src/secondary.rs#L584)
- [crates/sepika-load/src/secondary.rs::test_offset_beam_is_not_covered](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-load/src/secondary.rs#L618)

### Job

#### C102 — MERGE

**理由:** X/Y と XY の wave 配線を別 setup。

**失う検出・代替保証:** 方向別 acceleration の取り違え、XY の片側欠落。

**具体案:** X/Y/XY の expected arrays を table 化。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-job/src/sample_wave.rs::build_ground_motion_routes_by_direction](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-job/src/sample_wave.rs#L93)
- [crates/sepika-job/src/sample_wave.rs::build_ground_motion_xy_duplicates_wave](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-job/src/sample_wave.rs#L105)

#### C103 — MERGE

**理由:** 同じ static response の optional QL を別 setup。

**失う検出・代替保証:** 未指定 QL が勝手に推定される・明示値を無視する不具合。

**具体案:** None / Some(50000) を 1 table 化し response mapping も共有。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-job/src/ultimate_demand.rs::static_none_leaves_ql_unset](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-job/src/ultimate_demand.rs#L97)
- [crates/sepika-job/src/ultimate_demand.rs::static_explicit_ql](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-job/src/ultimate_demand.rs#L114)

#### C104 — MERGE

**理由:** 同じ dynamic mass 未算定を entry ごとに同じ fixture で拒否。

**失う検出・代替保証:** 入口のいずれかが旧 seismic_weight へ危険な fallback する不具合。

**具体案:** 各 entry は削らず、共通 model に対する runner table で全 entry の error を保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-job/src/lumped_mass.rs::planar_linear_requires_dynamic_mass](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-job/src/lumped_mass.rs#L631)
- [crates/sepika-job/src/lumped_mass.rs::spatial_linear_requires_dynamic_mass](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-job/src/lumped_mass.rs#L663)
- [crates/sepika-job/src/lumped_mass.rs::spatial_nonlinear_requires_dynamic_mass](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-job/src/lumped_mass.rs#L774)

### Solver

#### C105 — DELETE

**理由:** n_indep<n_free だけの smoke。運動学/連鎖のより強いテストが減約と出力変位を検査。

**失う検出・代替保証:** test_rigid_diaphragm_master_recovers_translation_and_torsion / test_chained_mpc_constraints_compose_transitively が同じ通常分岐と変位係数を検出。

**具体案:** 削除。fixed-master guard は独立なので保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-solver/src/common/constraint.rs::test_rigid_diaphragm](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-solver/src/common/constraint.rs#L483)
- [crates/sepika-solver/src/common/constraint.rs::test_mpc](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-solver/src/common/constraint.rs#L661)

#### C106 — SIMPLIFY

**理由:** n_indep<n_free のみで link の offset/回転係数を検証しない。

**失う検出・代替保証:** constraint 消失を見つけるが誤った係数でも通る。fixed-master のゼロ検査だけでは moving master の offset は守れない。

**具体案:** moving master の独立変位/回転検査へ統合。単独数減少 smoke は削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-solver/src/common/constraint.rs::test_rigid_link](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-solver/src/common/constraint.rs#L497)

#### C107 — SIMPLIFY

**理由:** 内部 heuristic 式の 10 値をコピー。小さい q の精度は dense reference テストが検証。

**失う検出・代替保証:** requested 超過・zero・q 切替時の不正 dimension が実害。常に同じ heuristic 数を返すこと自体の価値は低い。

**具体案:** q<n の dense 解比較と requested/n/zero の外部契約へ境界例を移し、中間の内部 q の固定値を削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-solver/src/dynamic/eigen/tests.rs::test_subspace_size_table](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-solver/src/dynamic/eigen/tests.rs#L1101)

#### C108 — DELETE

**理由:** identity 行列は elimination を通らず RHS をそのまま返すだけ。実際の 2DOF omega / time history が非対角系を通る。

**失う検出・代替保証:** Thomas 法の実際の elimination の誤りは test_fundamental_omega_sdof / test_lumped_mass_eigen_matches_power_iteration_omega1 / 時刻歴数値テストで検出。

**具体案:** 削除。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-solver/src/dynamic/lumped_mass/mod.rs::test_solve_tridiagonal_identity](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-solver/src/dynamic/lumped_mass/mod.rs#L148)

#### C109 — MERGE

**理由:** 同じ三折線 fit の単調性を別形状の fixture で再確認。

**失う検出・代替保証:** 折点重複や K2/K3 の負値・順序の誤り。

**具体案:** test_fit_trilinear_equal_area_and_endpoints に distinct folds と傾き順の検査を移し、ほぼ同じ軟化曲線の fixture を削る。bilinear/degenerate は維持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-solver/src/dynamic/lumped_mass/mod.rs::test_fit_trilinear_k2_k3_helpers](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-solver/src/dynamic/lumped_mass/mod.rs#L76)

#### C110 — SIMPLIFY

**理由:** 名前は方式が reference を変える保証だが、3方式とも mu>=1 と有限だけで選択の効果を確認していない。

**失う検出・代替保証:** method を常に FirstYield にしても通る可能性がある。

**具体案:** 方式別の独立 reference / ductility 検証へまとめる。現在の弱い共通下限のために 3 回 solve しない。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-solver/src/nonlinear/pushover/tests.rs::test_pushover_ductility_method_selection_changes_reference](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-solver/src/nonlinear/pushover/tests.rs#L616)

### I/O

#### C111 — SIMPLIFY

**理由:** 本番と同じ SHA256 ライブラリ呼出しで期待値を作成。

**失う検出・代替保証:** wave の実際の bytes と hash の対応・登録後の内容違い。SHA256 アルゴリズムの再確認は不要。

**具体案:** 登録/overwrite の fixture に独立既知 hash または内容変更で hash が変わる保証を統合。hash library の二重呼出しを削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-io/src/wave_library.rs::test_wave_sha256_matches_direct_hash](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-io/src/wave_library.rs#L164)

#### C112 — MERGE

**理由:** grade 解決の representative を wrapper/core 比較と固定値で二重記述。

**失う検出・代替保証:** import した grade と物性が別 grade にすり替わる不具合。

**具体案:** 成功 grade / unknown の table に独立既知物性を保持し、core 再呼出しを期待値にしない。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-io/src/stbridge/import/material_std.rs::test_resolve_grade_sn490_is_325](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-io/src/stbridge/import/material_std.rs#L77)
- [crates/sepika-io/src/stbridge/import/material_std.rs::test_resolve_grade_wires_to_core](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-io/src/stbridge/import/material_std.rs#L85)

### MCP

#### C113 — DELETE

**理由:** 固定 server metadata 名だけを検査。

**失う検出・代替保証:** 文字列変更の検出のみ。tool schema / result の機能検証とは独立した低価値確認。

**具体案:** 削除。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-mcp/src/server.rs::server_name_is_sepika_mcp](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-mcp/src/server.rs#L331)

#### C114 — DELETE

**理由:** 同じ cantilever の LinearStatic job を stronger persistence test より先に再実行。

**失う検出・代替保証:** test_linear_static_job_persists_and_result_get_filters_nodes が job completion と結果出力を検出。

**具体案:** 削除。失敗系・eigen/pushover/design の別 job 経路は保持。

**費用:** async job と temp store の重複実行・待機を削減。

**対象:**

- [crates/sepika-mcp/src/server.rs::test_analysis_run_completes_for_valid_model](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-mcp/src/server.rs#L639)

#### C115 — DELETE

**理由:** helper の notice は実際の async job の notices 結果で再確認される。

**失う検出・代替保証:** test_analysis_run_semi_precise_without_design_period_completes が EX/EY notice の配線と内容を検出。

**具体案:** 単独 helper テストを削除。

**費用:** 短時間だが helper→外部結果の二重維持を減らす。

**対象:**

- [crates/sepika-mcp/src/server.rs::test_prepare_notices_for_semi_precise_without_design_period](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-mcp/src/server.rs#L844)

#### C116 — SIMPLIFY

**理由:** parse 成功後に command.label() だけ assert し、読み込んだ member / ends を確認しない。

**失う検出・代替保証:** JSON フィールドを無視しても同じ command type の label で通る。

**具体案:** parse→apply_edit の既存/共有 fixture で member / ends / end_support の外部状態を確認し、label-only 検査を削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-mcp/src/tests.rs::test_parse_set_secondary_member_end_support](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-mcp/src/tests.rs#L958)
- [crates/sepika-mcp/src/tests.rs::test_parse_place_secondary_member](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-mcp/src/tests.rs#L997)
- [crates/sepika-mcp/src/tests.rs::test_parse_set_secondary_member_ends](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-mcp/src/tests.rs#L1024)

#### C117 — MERGE

**理由:** query の filter/unknown の短い同一 fixture。

**失う検出・代替保証:** filter の無視・unknown kind の誤解釈。

**具体案:** kind/filter を table 化。nodes/elements の異なる field mapping は別契約として保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-mcp/src/tests.rs::test_query_model_filter](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-mcp/src/tests.rs#L176)
- [crates/sepika-mcp/src/tests.rs::test_query_model_unknown_kind](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-mcp/src/tests.rs#L184)

### Section

#### C118 — SIMPLIFY

**理由:** 単純 field copy と重要な SectionId 配線が混在。

**失う検出・代替保証:** catalog→section で id/形状/物性が誤る不具合。

**具体案:** catalog 追加の edit roundtrip に代表既知物性と id の確認を集約。単純代入専用 fixture を削る。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [crates/sepika-section/src/catalog.rs::test_to_section_carries_name_and_properties](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-section/src/catalog.rs#L297)

### xtask

#### C119 — MERGE

**理由:** 同じ markdown link parser の短いケースの分割。

**失う検出・代替保証:** local/remote/image/anchor の分類や anchor 除去の誤り。

**具体案:** 入力/リンク列の table 化。各種入力は保持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [xtask/src/check_docs.rs::extracts_relative_md_links](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/xtask/src/check_docs.rs#L253)
- [xtask/src/check_docs.rs::strips_anchor_from_md_link](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/xtask/src/check_docs.rs#L259)
- [xtask/src/check_docs.rs::ignores_https_links](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/xtask/src/check_docs.rs#L265)
- [xtask/src/check_docs.rs::ignores_anchor_only_links](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/xtask/src/check_docs.rs#L270)
- [xtask/src/check_docs.rs::ignores_image_links](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/xtask/src/check_docs.rs#L275)

#### C120 — MERGE

**理由:** fence state の異常/正常を短い別 fixture。

**失う検出・代替保証:** fence の種類・長さ・blockquote により架空の参照を検出する不具合。

**具体案:** markdown_links / impl_refs の 2 出力を fence table で共有し、全 fence 状態は保持。ファイル存在検査は parser の代わりにしない。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [xtask/src/check_docs.rs::ignores_fenced_content](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/xtask/src/check_docs.rs#L280)
- [xtask/src/check_docs.rs::ignores_tilde_fenced_content](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/xtask/src/check_docs.rs#L287)
- [xtask/src/check_docs.rs::does_not_close_backtick_fence_with_tildes](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/xtask/src/check_docs.rs#L294)
- [xtask/src/check_docs.rs::does_not_close_longer_fence_with_shorter_run](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/xtask/src/check_docs.rs#L301)
- [xtask/src/check_docs.rs::ignores_blockquoted_fenced_content](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/xtask/src/check_docs.rs#L308)

#### C121 — MERGE

**理由:** 同じ missing link の file と directory の setup 重複。

**失う検出・代替保証:** directory を有効 markdown として扱う不具合。

**具体案:** 実 filesystem fixture の table とし、存在しない file / directory の両条件を維持。

**費用:** 実行短縮より、fixture と期待値の二重管理・変更追従の削減が主な効果。

**対象:**

- [xtask/src/check_docs.rs::detects_missing_markdown_file](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/xtask/src/check_docs.rs#L349)
- [xtask/src/check_docs.rs::treats_directory_as_missing_markdown_file](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/xtask/src/check_docs.rs#L366)

### 計測

#### C129 — SIMPLIFY

**理由:** ignore 指定済みの手動計測。通常 regression の件数には保証として含めない。

**失う検出・代替保証:** 計測 harness の利用可能性だけ。通常テストの失敗検出保証はない。

**具体案:** 将来整理時に手動 benchmark / xtask へ移す。ignore なので通常の実行時間短縮はない。

**費用:** 通常実行の費用ゼロ。役割の明確化と test-target のビルド/配置負担のみ。

**対象:**

- [crates/sepika-app/tests/perf_probe.rs::perf_probe_generate_stories_action](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-app/tests/perf_probe.rs#L452)

#### C130 — SIMPLIFY

**理由:** ignore 指定済みの手動性能 probe。

**失う検出・代替保証:** 計測手順の利用可能性のみ。通常 regression 保証はない。

**具体案:** 手動 benchmark / xtask へ移す。性能調査機能を捨てない。

**費用:** ignore により通常実行の費用ゼロ。

**対象:**

- [crates/sepika-core/tests/perf_probe.rs::perf_probe_wall_planes](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-core/tests/perf_probe.rs#L121)

#### C131 — SIMPLIFY

**理由:** ignore 指定済みの実モデル codec 比較測定。

**失う検出・代替保証:** 計測 harness の利用可能性のみ。codec は通常 roundtrip が保証。

**具体案:** 手動 codec benchmark へ移し、通常の roundtrip / schema 検査を維持。

**費用:** ignore により通常実行の費用ゼロ。

**対象:**

- [crates/sepika-io/src/ovika.rs::measure_real_model_msgpack](https://github.com/hrntsm/SEPIKA/blob/3286fbcb45a3cc7e296a839a5488d3a3b42a9307/crates/sepika-io/src/ovika.rs#L532)

## 検証

- GUI / MCP / Parquet 有効の `cargo test --workspace --features sepika-app/gui,sepika-mcp/mcp,sepika-io/parquet --locked -- --list` が完了。3,052通常関数＋2 rustdoc と source 一覧が一致。これは実行テストの成功を意味しない。
- 隔離コピーの commit/revert no-op 実験を実行（前記）。
- 全件CSVの一意性・候補ID・基準ソース行・候補本文・全source testとの一致を検証する。

## 実装との対応

この文書は整理前の監査スナップショットであり、すべての候補を削除済みという意味ではない。実装時は変更したグループと移した保証を別途記録する。大規模な production refactor を必要とする提案はテスト整理のためだけに実施しない。
