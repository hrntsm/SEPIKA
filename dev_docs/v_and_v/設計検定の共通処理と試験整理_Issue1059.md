# 設計検定の共通処理と試験整理（Issue1059）

対象基点は `86bc0808a3c0671c09b18b2c86bc49139d5bf31b`。現行の設計検定にある不要な経路・所有・反復と、同一入力の試験を整理する。検定式・係数・演算順・診断順・公差を変更しない。固定SHAの判断表と過去の実行証跡は、その版の履歴として保持する。

## 現役の保証

| 元の試験・観測 | 現役の試験・観測 | 保持する入力・独立期待値・公差 |
| --- | --- | --- |
| `vnv_rigid_zone_mechanism::剛域つきプッシュオーバーが解析上の機構に達する` | 同ファイル `剛域は保有水平耐力を可撓長基準へ引き上げる` | 剛域300 mm・重量600000・400stepの同じ結果へ、非Crackヒンジ総数4以上と機構enum非Partialを移設。固有材端数4以上と混同しない。剛域なし0 mm・同重量/step、可撓長比1.25に対する5%公差、未降伏1000・20stepを保持 |
| `quantity::tests::{test_rc_portal_categories_and_concrete,test_rc_girder_main_bars_and_stirrups,test_rc_column_rebar}` | `test_rc_portal_categories_concrete_and_rebar` | 同じ既定portal一回の数量算定へカテゴリ・コンクリート・型枠・全小計と、大梁/柱の主筋長・重量・補強筋長・継手の全assertを移設。1e-9/1e-12を保持。断面用途不足・編集後fixtureは別試験のまま |
| `rc::beam::tests::{test_beam_check_via_design_check_trait,test_beam_check_components_bending_and_shear,test_beam_check_detail_fragments_assigned_to_intended_components}` | `test_beam_check_trait_reports_ratio_basis_components_and_details` | 同じRC梁300×600、Fc24/SD345、Long、pos0、Qy20000、Mz30000000の結果へ全assertを移設。正検定比・13条basis・component有無・MA_t/QA/共通atの配置を保持。長さと付着条件が異なる試験は別のまま |
| `srrc::column::tests::test_src_column_n0_matches_smo_plus_rm0` | `test_src_column_biaxial_linear_sum` | 同じSRC形状・Fc24/SD345・ColumnLong・Mz1からのMAを共用。正値/有限性と鋼骨曲げ耐力の0.99倍以上の比較を保持。MA×0.3の需要と0.05公差も保持 |
| `ultimate::tests::{test_ultimate_beam_rect_tension_side_affects_mu,test_ultimate_beam_rect_top_tension_reduces_qbu}` | `test_ultimate_beam_rect_tension_side_affects_mu_and_qbu` | 同じ非対称RC梁・上下Mz±1e8・既定optsの両結果を共用。Mu正値/差1超/上端>下端とQbu両正値/上端<下端をすべて保持 |
| `ultimate::tests::test_ultimate_check_pushover_demand` の二度目のRp0算定 | 同試験の最初の `checks`/`col` | Elem0の軸力1e6/Mz1e8/My5e7/Qm123456/Rp0、Elem1の全0が同一。Qmuの1e-3公差、Rp0.03比較、axialのみ・弱軸需要は保持 |
| `srrc::panel_zone::tests::src_panel_hand_calc` | `src_panel_cv_uses_full_cb_regardless_of_beam_type` | 同じ既定入力、全幅Cbの期待式と1e-6を保持。beam_is_steelのfalse/true比較は1e-12のまま |
| `steel::beam::tests::test_beam_check_fb_basis_asd2019_lb_zero_equals_ft` | `steel::section::tests::test_asd2019_fixed_values_cover_elastic_inelastic_and_caps` | 同じhelperのF235・lb0・Long、固定156.66666666666666と1e-9を保持。Shortの固定235と1e-9も保持。元試験はbeam dispatchを呼ばない |
| 材料Fc24のdirect shear、Rs/Re代表値 | 既存の `concrete_shear_922_matches_independent_legal_boundary_values`、`fes718_normal_values_and_invalid_inputs` | 同じ全入力・期待値を1e-12で保持。compression/class/lightweight、cap/不正値の別入力は保持 |
| RC接合部有効幅の循環期待値 | `rc::joint::tests::rc_joint_bj_uses_min_of_bi_half_and_d_quarter` | 元の柱幅1200 mmを残し、同じ需要・材料の幅600 mmと比較。独立幾何値bj450/600 mmから検定比wide/base=0.75を1e-9で観測。minをmaxへ変えると0.8となるため区別できる |

## source整理と削除の境界

- 壁集約は借用iteratorを共有し、kind filter、耐震対象外、壁key、空component、NG/未検定優先順と最大値の処理順を保持する。
- 床梁は一呼出の転送関数を入口へ展開し、先頭の絶対値化と全validation/理由文言を保持する。steel柱は既に同じ入力で算定したせん断面積だけを再利用し、H形ウェブ応力の乗除順を変更しない。
- RC柱の先頭Circle/Rect分岐はそれぞれreturnする。その後の同形状の二重実装のみを除去し、対象外形状のpanicを保持する。不要になったprivate引数だけを呼出元と合わせて除去する。
- `RcRebarInfo.main_count_per_side` はcrate-private・非serdeで、constructorと自己assert以外の消費がない。fieldとその4assertのみを除き、全形状・円形6/8本・実消費propertyの保証を保持する。数量の単一variantラッパーと未使用材料引数、円形だけが到達するN–M転送関数も除去する。
- CFT材料Fc、鋼断面j/grossの不変assertは、直接fieldをimmutable借用した後の型保証の再確認のみを除去する。全入力族・検定結果・診断は保持する。
- 開口面積は独立literal1250000を用いる。local算術だけのMe/λ/ν・scallop・spacing・sNu確認、literal filled、正確なequalityに包含される不等値を除く。実装出力の固定oracleと公差は保持する。
- eccentricityの全field同値分岐と、joint fixture既定Fc17/pwh0.015の再代入・元々ない要素3/4へのretainのみを除く。

## Rustdocと確認の限界

終局RCせん断・靭性せん断・CFT軸耐力/N–Mの完全式や算定経緯は既存の[RC終局せん断](../../docs/calc_basis/08_部材終局耐力/02_RC_終局せん断.md)・[CFT柱終局耐力](../../docs/calc_basis/08_部材終局耐力/06_CFT_柱_終局耐力.md)に掲載済みである。Rustdocは単位・前提・戻り順・不正入力時挙動に絞り、圧縮側主筋面積acの対称仮定という誤記を訂正する。正本の式・制約は変更しない。strict Rustdocが検出した既存8行の単位の誤リンクは、単位文字列と契約文言を維持してinline codeへ修正した。

文書配置の同一項目alias008には、次の全対象も含む。完全式・内部How・整理経緯だけを除き、計算本体・試験の入力/assertは変更していない。

| 対象 | 契約として保持する内容と既存の正本 |
| --- | --- |
| `steel/section.rs` の `steel_i_t`、`steel_lateral_buckling_i_af`、`steel_fb_h`、`steel_lateral_buckling_c`、`end_moment_ratio_m2_m1`、`steel_c_factor`、`steel_fb_h_asd2019`、`steel_p_lambda_b`、`steel_warping_constant`、`steel_h_z_with_loss`、`resolve_lb` | 単位・戻り順・shape選択・C直接入力優先・C/pλb既定・端部比符号・横座屈長さ入力優先/位置区間・端部欠損適用。式と制約は[鋼構造の正本](../../docs/calc_basis/06_一次設計/03_鋼構造_断面検定.md#許容曲げ応力度と横座屈)、GUI非提供1973式は[GUI改善Issueの計算資料](https://github.com/hrntsm/SEPIKA/issues/1166)へ集約 |
| `quantity/mod.rs` の出力単位・数量用単位重量、矩形/円形配筋helper、壁内法寸法、壁版数量と `quantity/tests.rs` の壁版2試験Rustdoc | 出力単位、固定荷重と数量の単位重量の用途区別、対象・寸法単位・配筋未入力時不変更・内法戻り順/対象外・壁区分/断面符号/鉄筋未算定状態。[数量正本](../../docs/calc_basis/10_数量積算/01_数量積算の算定式.md)の既存式・全本数/周長・未算定制約を保持 |

bj試験は中央取付き有効幅minのVerificationであり、許容式全体のValidationではない。κA/fs/Qdjの規準本文未照合・設計採用保留、RCせん断のkst代表式全域採用と円形換算の安全性未確認、CFT長方形準用等の制約を維持する。原典照合や新たな安全側判定は行わない。半径None/明示0、Add/Edit/SetRadius/Catalogの拒否と原子性を検証する対象外試験は変更していない。

Issue1059は全840ファイル読了後も残候補と全台帳の独立最終確認を必要とし、このまとまりだけで完了としない。

## ローカル検証

基点とworking tree差分に対し、`cargo +stable test -p sepika-design-jp --locked`はunit687件・integration27件、計714件成功（失敗・ignoredとも0、Rustdoc testは0件）。全target Clippy（`-D warnings`）、`RUSTDOCFLAGS=-D warnings`のRustdoc、fmt、check-docs（summary141/link863/impl ref429）とmdBook buildも成功した。packageに独自feature定義はなく、全testには既存integrationとRustdoc testを含む。実建物/実験Validation、手動GUI、規準適合の追加確認は行わない。

文書修正Feedback後は、対象3ファイルからRustdoc行と空行を除いた全行の完全一致を確認した。全714件の検証対象とactive body/assertが同一のため試験の再実行はせず、strict Rustdoc（4.25秒）・fmt・check-docs（141/863/429）・mdBookを再実行し成功した。
