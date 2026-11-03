# 共有入力と反復処理の整理（Issue1059）

初回検証基点は `86bc0808a3c0671c09b18b2c86bc49139d5bf31b`。非Appの38候補と検証で発見した単位誤リンク1候補（3行）について、不要な所有・反復・到達不能分岐と同一fixtureの検証を整理する。構造式・係数・浮動小数演算順・診断順・入力・独立期待値・公差を維持する。以下は現役の対応であり、固定SHA判断表・rawログ・過去の実測値はその版の履歴として保持する。

## 現役の保証

| 対象・元観測 | 整理先と保持する保証 |
| --- | --- |
| core `material_properties_converge_as_radius_steps_shrink` | 3形状×境界0/16×eps0.1/0.01/0.001の左右props配列を各一回保持。10成分、前誤差比0.2と最終相対1e-5は全保持。材料強度係数は各名称/直接1.07/既定を残し、共有借用後のfy不変比較のみ除去 |
| material `HysteresisMaterial::evaluate` | reversed両分岐の同じdir代入を共通末尾へ。途中にdir読取・early returnがないことを確認し、分岐・履歴更新順を保持 |
| MCP `beam_contact_quantity_mcp_preserves_union_and_diagnostics` | 非mcp構成は直接JSON試験を保持。mcp構成は既存 `beam_contact_quantity_mcp_tool_returns_verified_rows_and_unavailable_reasons` が同じ全fixtureを実handler経由で観測。7.5/8.4/8.1・各配置期待値と1e-9、全不正理由/unavailable/totals=nullを保持 |
| MCP SlabPlate/query集計 | region、anchor、extentの先行検証後に共通parserを呼ぶ。plateの欠落とnullを区別し、fallbackのsection→loads→usage→method→one_wayと診断を維持。数量totalsは同じ不変結果から一回集計し表示値・加算順を保持 |
| job `compute_time_history` | 同じeigen(1)→先頭正値sqrt→エラー分類を共有。各減衰分岐の元位置で一回呼び、Rayleigh/Modal分岐・モデル・診断・解析順は変更しない |
| job壁荷重3試験 | `compute_gravity_includes_attached_wall_plate_self_weight` / `compute_gravity_preserves_trapezoid_linear_shape_on_beam` / `compute_gravity_includes_floor_region_attached_wall_as_slab_extra` の同じFc24材料・壁t150断面だけを共有。矩形/台形/床領域の各入力、荷重伝達・総和・強度比と各公差は保持 |
| IO `stb_negative_section_reference_is_rejected_for_all_linear_member_types` | `stb_whole_document_rejects_invalid_present_member_reference_and_source_ids` がColumn/Girder/Brace/Beam/Postの同じS・有効端点・id_section=-1を両公開importで拒否する。field/range理由、他の不正ID/欠落/原ID/板入力は保持 |
| IO `test_roundtrip_semantic` | `test_roundtrip_twice_stable` の初回import直後へ元の `assert_semantic_eq(&m,&m2)` を移設。代表モデル・初回/二回目意味保存・XML安定をすべて保持 |
| IO rebar/exportの重複assert | 直前の `assert_rc_rebar_ovika_contract` が同じshape比較を含む3件、連続した同一 `assert_unassigned_625` 2組と固定座標のlocal面積assertだけを除去。独立XML、OVIKA全保存、validate、両公開export拒否/診断、import面積/支持/区間/負担率は保持 |
| element壁質量3試験 | `test_wall_mass_and_column_properties_preserve_section_mass_with_rebar` へ集約。元4000×3000×150の同じfixtureで、実参照鉄筋密度正値・独立体積質量相対1e-12・単位長質量/両回転慣性/極慣性1e-12・Consistent/Lumped全成分和と非対角差を全保持。異なるFc/材料不正入力は別試験 |
| element剛性/パネル/壁 | raw剛性で未読のclone上Fixed再代入のみ除去。正の候補厚からの到達不能fallbackのみ除き建物一律厚の概念は保持。パネルpreflightの原子的拒否とnode順序を保持しnode ID Vecだけ除去。壁の最初の断面診断とRC早期return後の鋼材検証、密度選択とuniform引数順は維持 |
| element fixture/拒否 | `rc_col` はx/yだけの配筋有無を選び全field同値。beamのshould_panic呼出と期待理由は残し、panic後の到達不能assertだけ除去。seg_a/seg_bのねじり解放assertと剛性は残し末尾重複だけ除去。側柱試験の古いpenalty説明のみ除去 |
| solver `validate_entries` | 全要素の個数/順序/DOF整合を保持し、荷重所有要素にだけ必要な物理節点集合のcollect/sort/dedupを分岐内へ。所有要素の物理節点不一致も引き続き拒否 |
| solver識別/重量拒否 | 同じCURVE/options1.05・0.5の拒否は `insufficient_interval_no_root_and_nonunique_area_remain_distinct` に残す。ダンパー編集2/3/4の同一条件は `同階合計を保つ編集でもダンパーの重量配分と配置の変更は全公開solverで拒否する` が両質量方式×同期有無で包含し、例外種/診断完全一致も保持。元の他5編集と正常質量4.0の1e-10は保持 |
| solver `step_record` | 同じ不変model/caseの最初の線形結果を後段層せん断へ再利用。変位・抵抗・断面力・層せん断の全assertを保持 |
| solver RC壁剛性/反力/仕事 | `rc_wall_stiffness_forces_reactions_and_work_match_independent_reference` へ同KINDS/Ux/100000の結果を共有。独立変位/剛性・力分担・反力・仕事の全固定値と公差を保持。仕事積分用0/50000/100000の別状態求解は変更しない |
| solverパネルM=Kγ/残差 | `test_panel_shear_angle_and_dof_equilibrium_match_closed_form` で同じl_frame(true)の解を共有。各成分M=Kγと、外力0/全体系正規化残差1e-9は別々のassertとして保持 |
| solver剛床3試験 | `test_rigid_floor_linear_results_and_missing_force_diagnostics` で同じrigid_floor_portal(true)/case1を共有。全線材内力回収・中央曲げ範囲、false側の全断面力/変位比較と既存公差、欠落Elem2の拒否/ID診断を全保持 |
| solver共通断面質量fixture | 既存 `tests/support/wall_side_column.rs` のrebar/fixtureへ同値入力構築のみを共有。`common_section_mass.rs` 内の独立質量・慣性期待値と各公差は移動しない |
| solver曲げ質点系 | 既存 `time_history::story_spring` を再利用。同じMaxPointOrientedの3折点・材料・初期状態・確定順を保持 |
| solver説明 | th_benchの巨大record_everyでも初期/最終frameと毎step包絡/ピーク集計は残ると訂正。所属未解決小梁と壁厚不足の試験説明を既存Error/precheck拒否assertへ一致させる。ベンチ入力・計算・既存測定値は変更しない |

## 信頼境界と在庫

STB読込はBOMの独立UTF8処理を維持し、通常UTF8判定で所有bytesを使い、失敗時だけ元bytesを回収してShift_JISへ渡す。デコード理由と順序は不変。OVIKAはpreparation/results/settings/studiesの同じoptional entry配列をmanifestとZIPで共有し、順序・None除外・bytes/hashを保持する。RawNode/RawStoryの重複ID拒否を維持し、同じRawStory列由来で既に成立するStory.node_idsへの無操作補完だけを除去する。

未参照 `crates/sepika-io/tests/fixtures/public_strength_material.stb`（SHA256 `25956898a9775ee1dff8a476b75e52b3127d145f0f762fcdb53eec18dfc9def6`）を削除した。配筋containerを除いた `public_strength_bars.stb` とXML全体が一致し、現役の[標準強度受入](標準強度_Issue520_2026-10.md)はbarsとstrength_srcを使用する。元fixtureは上記固定基点のgit履歴に保持する。sourceファイルの追加・削除はなく、共有fixture込み対象在庫は840から839となる。

strict Rustdocが検出した `dynamic/lumped_mass/model.rs` の変位単位と `statics/linear/mod.rs` の層せん断単位と `job/settings.rs` の層間変形角単位は、単位文字列を変えずinline codeへ修正した。計算本体・入力・assertは不変で、対象在庫839を維持する。

## 検証と限界

最初の6crate defaultは上記86bc0808基点で2085件成功・失敗0・既存ignored4。その後mainを `f32c39a9d10c293f44ee44bee65d43fa1422d1c4` へ更新し、checkpoint差分のrange-diff同値を確認した。更新後はMCP default106件、mcp feature143件、IO parquet248件が成功（失敗0、IO parquetは既存ignored2）。未使用importの整理と単位のRustdoc修正はactive body/assert不変。

7crate all-target ClippyとMCP feature Clippy（ともに `-D warnings`）、7crate strict RustdocとMCP feature strict Rustdoc、fmt、専用targetのcheck-docs（141/863/429）、mdBook、V&Vローカルリンク確認も成功した。IO defaultはMCPによるparquet feature統合と分離して確認する。既存のignored計測/外部XSDや実GUI操作を実施したとは扱わない。

この整理は既存Verificationを保持し、実験Validationや新しい原典照合・安全側判定を追加しない。半径None/明示0、Add/Edit/SetRadius/Catalogの拒否と原子性を検証する対象外試験は変更しない。#1160/#1161の調査保留、#489/#1162のnot_plannedを再開せず、Issue1059全体は残App実装と全台帳の独立最終確認まで未完了とする。
