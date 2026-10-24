# 大梁床板実重複控除の確認記録（Issue #616）

## 対象と根拠

矩形RC/SRC主架構大梁の設計自重について、床板の実断面・実配置と明示支持関係を使う控除をVerificationする。
現在の算定方式・範囲は[製品仕様](../../docs/calc_basis/01_荷重/06_自重の長期応力解析への接続.md#矩形rcsrc大梁と床板の実重複控除)を正とする。
以下の期待値はIssueに確定した寸法・独立算術によるものであり、他の製品や未確認資料による照合ではない。
変更前はコードで共通厚・床領域存在による一律控除を確認した。変更前バイナリの数値再現は未実施。

## 独立期待値と実入口

`crates/sepika-app/src/app/girder_weight_tests.rs` の `girder_overlap_independent_values_reach_preparation_dl_and_seismic_weight` を使用する。
梁幅400mm・せい700mm・採用自重長6000mm・設計単位重量24kN/m³。FL=3000mm、ΔSL=−350mm、節点高さ2650mmとし、梁偏心で上端を作る。床上下面はFLと床偏心・実厚から作り、各ケースで支持先は変えない。

| 梁上端のFL差 | 左床厚/上面のFL差 | 右床厚/上面のFL差 | 梁本体の控除後重量 kN | 結果 |
| --- | --- | --- | ---: | --- |
| 0 | なし | なし | 40.32 | 成功 |
| −200 | 150/0 | なし | 40.32 | 成功 |
| −50 | 150/0 | なし | 34.56 | 成功 |
| 0 | 150/0 | なし | 31.68 | 成功 |
| 0 | 200/0 | なし | 28.80 | 成功 |
| −50 | 150/0 | 200/0 | 33.12 | 成功 |
| −50 | 150/0 | 150/+400 | 37.44 | 成功 |

左右交換した各ケースも同じ結果。許容絶対差は1e−6 N。
DL自重内容生成・直接階生成・実際の `App::run_preparation` を通し、準備結果の梁本体/控除の内訳を照合する。
準備後の実DL総合力・階地震重量には床板本体が含まれるため、各12m²の床板の重量を独立算術 `288×板厚[mm] N` で分離して照合する。
嵩上げは加算されていない。CSVの内訳列の存在も確認する。

## 幾何・適用条件・更新

- NoPlateによる片側全幅、重複0の床板が存在する場合の半幅保持、共通厚0/25/1000/NaNでの控除不変、900mm床板と700mm梁せいの交差を確認する。
- 同じ床板候補の重複収集で控除を増やさず、同じ側の異なる床板候補はエラー。別場所の床板は無関係とする。
- 左右の別領域から同じ床板IDを参照した場合は、割当参照検査とDL入口の双方で拒否する。床板は梁で囲まれた閉領域ごとに別の実体であり、複数領域の共有を有効モデルとして追加しない（2026-10-24の語彙確認）。
- Unset、参照切れ、梁断面の未割当と存在しない断面ID、階・床断面・材料欠落、偏心欠落/非有限、板厚の不正、梁幅の不正、位置・梁材料密度の非有限、支持の不明・部分支持をDL入口と直接算定で拒否する。
- 床板材料の密度を梁の半分にしても、梁本体の単位重量による控除量は不変である。
- Column/Brace用途とS断面に拡張しない。RCとSRCの設計重量、明示Fiber/MultiSpringを確認する。
- 両端の柱フェース各150mmにより採用長5700mmとなる場合、梁本体38.304kN、控除後30.096kNを独立値とする。
- 対象大梁への取り付く床板は未対応理由付きで停止し、別の梁に取り付く床板では対象大梁の控除を変えない。
- 幅方向が水平でない断面回転は未対応として停止し、局所座標の丸め範囲は正常対照とする。
- 梁偏心、床偏心、床断面形状の厚さ、梁の材料密度、NoPlate割当、節点位置を編集する。結果入力識別と失効、Undo/Redo、再準備、OVIKA保存再読込による重量を確認する。
- Unsetへ戻した後は準備結果を公開せず、生成重量の入力キーを不一致、算定済み重量一覧を空にする。手入力を保持するため、階の保存表示値をすべて削除することは要求しない。

### 斜梁の独立値

梁の水平長6000mm・高低差4500mmの3:4:5勾配では材軸長7500mm、梁本体50.4kN。
上記梁の始端中心2650mm、床板上面3000mm・厚150mmの場合、床との交差高さは水平座標0〜850mmで150mm、850〜1050mmで150mmから0へ直線変化する。
水平断面積分は142500mm²、幅400mmを掛けた控除体積57000000mm³、控除1.368kN、控除後49.032kN。

全梁の上下を覆う床板では、中央の鉛直厚875mmを使う。材軸方向の両端262.5mmでは有限端面により鉛直厚が437.5〜875mmへ変化する。
水平投影での控除体積は2063250000mm³、控除49.518kN。
この値は断面せい700mmへのclamp・材軸長の直接使用・有限端面無視を区別する。i/j反転でも控除総量不変を確認する。

## 保存生成重量の計算契約

解析契約を45から46へ更新し、重量生成入力キーに解析契約版を含める。
`sepika-job/tests/seismic_freshness.rs` の `old_calculation_contract_and_unversioned_weight_keys_force_regeneration` で、旧版および版を持たない生成キーを拒否し、再準備後の重量と鮮度が復元されることを確認する。

## 実行記録

2026-10-24、Windows、stable rustc 1.99.1 / clippy 0.1.99（2026-10-13）。`rustup update stable` は更新なしで成功。

| コマンド・対象 | 結果 |
| --- | --- |
| 初回 `cargo test -p sepika-core -p sepika-load -p sepika-job --locked` のcore各群 | unit589件と統合41件成功、既存1件ignored。コマンド全体は後続jobの旧fixtureで失敗 |
| `cargo test -p sepika-load -p sepika-job --locked --no-fail-fast` | 修正後454件とdoc-test成功 |
| `cargo test -p sepika-app --features gui --lib --locked` | 917件成功 |
| `cargo test -p sepika-app --features gui --lib girder_weight_tests --locked` | 追加の床板密度・不正共有参照を含め9件成功 |
| `cargo test -p sepika-mcp --features mcp --lib public_mcp_ex_uses_edited_density_finish_and_seismic_live_without_prepare_button --locked` | 1件成功 |
| `cargo test -p sepika-mcp --features mcp --lib mixed_structure_basement_mcp_standard_load_matches_above_ground_period --locked` | 1件成功 |
| `cargo test -p sepika-mcp --features mcp --test member_load_direction --locked` | fixtureの偏心明示後1件成功 |
| `cargo clippy -p sepika-core -p sepika-load -p sepika-job -p sepika-app -p sepika-mcp --all-targets --features sepika-app/gui,sepika-mcp/mcp --locked -- -D warnings` | 成功。初回は新規試験の不要なto_vec・型複雑度の2件を検出し、試験の記述を修正 |
| `cargo run -p xtask -- check-docs` | summary124・links636・impl refs671、成功 |
| `cargo fmt --all -- --check` / `git diff --check` | 成功 |
| `mdbook build` | 成功 |

### 既存試験モデルの更新

既存の鮮度試験はRC大梁と取り付く床板を使っていたため、新しい適用条件による未対応停止で10件が失敗した。
鮮度の共通モデルを自重0のS大梁・RC柱・取り付く床板へ変更し、基準100kN/階と、RC柱の密度・床仕上げ・地震用積載の変更による重量変化を維持した。参照するapp/MCP試験も同じモデルを使う。
途中階を追加する試験の追加梁には有重量S断面を使い、0重量階にしない。上階のRC柱密度編集の独立期待値は102206.49625N。
SRCと荷重方向の保存fixture、STB配筋通知の試験では、準備前の梁偏心を明示0とした。床の分割と小梁CMQの試験は、RC大梁・RC小梁の元fixtureで、同側床群の全長被覆を検証する。
公開DL列挙で無断面・未割当材料の線材を自重0へ代用しない防護を加えたため、重量を除外していた床分配・反力・ダンパー試験には自重0の鋼材と実在断面を明示した。「断面・材料がなくてもRCとして成功」の既存試験は、欠落を理由付きで拒否する新しい契約へ更新した。
ダンパー総重量で置換する大梁は、採用しない自重内訳を表示せず、未設定床があっても総重量10kNによるDL・地震重量・準備が一致することを確認する。

## CI追補：同側床群による連続全長割当

[初回CI](https://github.com/hrntsm/SEPIKA/actions/runs/40092582330)では、workspace/feature両構成で `full_model` が16成功・21失敗となった。
取り込んだ梁偏心が未指定のため準備計算が停止していた。解析専用fixtureのFL/ΔSL・床板配置と同じ責務で、梁の節点・支持アンカーからの鉛直差0mmを明示した。原本と派生STBの取込直後、STB往復直後は未指定のままであることをassertし、importerへ既定0を追加していない。

その後、梁41の同側にある床板1/2/3が一律に矛盾扱いされることが分かった。実測した支持区間は材軸mmで `[0,2933]` / `[2933,5867]` / `[5867,8800]`。正長の重複はなく、既存の床板群が自重区間全長を覆う。
同側のID数だけでは同じ区間の矛盾と判定できないため、IDごとの区間和集合を統合し、全端点で区切った各開区間の候補を検査する。別IDの正長重なり・和集合の隙間・版なしによる部分被覆は停止する。取り付く床板や単独の部分取付きを補完する変更ではない。

同厚150mmの3床は分割前と同じ31.68kN。長さ1500/3000/1500mm、厚100/150/300mmの3床では独立体積計算で控除10080N、控除後30240N。反対側に厚150mmの全長床板がある場合は各半幅となり30960N。明示NoPlateの隙間、別IDの正長重複、単独部分床の拒否も検証する。
当初、分割床の既存CMQ試験をS大梁へ一時変更したが、この全長割当の解釈を訂正し、元のRC大梁・RC小梁へ戻した。

### 実モデルの独立内訳とsnapshot差

梁41は実軸 `(5000,1000,200)`〜`(13800,1000,200)`、b500・D1800mm、自重区間 `[200,8600]` mm。床板の上面200・下面50mmで、重複高さは150mm。
梁本体7560000000mm³、控除630000000mm³、γ=24kN/m³から控除15120Nを独立値とし、公開自重内訳と照合する。

同じRC基礎大梁17本について、b500mm・実重複150mm・γ24kN/m³による独立値は次のとおり。

| 梁ID | 採用長mm | 1本の控除N | 本数 |
| --- | ---: | ---: | ---: |
| 40/48/56/66 | 3650 | 6570 | 4 |
| 41/49/57/67 | 8400 | 15120 | 4 |
| 76/77/78 | 6100 | 10980 | 3 |
| 91/92/93/103/104/105 | 6000 | 10800 | 6 |

合計控除184500Nを恒久回帰で照合する。fixtureの共通厚は0であり、旧挙動の控除は0Nだった。
正常算定後のsnapshot差は `metrics[RFL].Re` の `5.020e-3`→`5.019e-3` の1行のみ。控除重量だけを一時的に0Nへ戻した対照実験で旧snapshotが成功し、新控除による差と切り分けた。本体は直後に復元し、独立内訳を確認したうえで1行だけ更新した。

`wall_model` は床板のない壁専用fixtureなのに水平領域が未設定だったため12件が停止した。版なしを明示すると16件すべてと従来のsnapshotが成功し、壁・柱・梁の材種や算定条件を変えていない。

### CI追補のローカル実行

`cargo test -p sepika-app --tests --features gui,sepika-io/parquet --locked --no-fail-fast` は1022件成功・既存の性能プローブ1件ignored。unit918件と全integrationを含み、`full_model` 38件・`wall_model` 16件が成功した。
元RC fixtureへの復帰後、`cargo test -p sepika-app --features gui,sepika-io/parquet --lib test_secondary_beam_subdivided_slab_dl_cmq_and_solve --locked` は1件成功。
新しい区間集計後の `cargo test -p sepika-load --locked` は283件とdoc-testが成功した。
`cargo clippy -p sepika-load -p sepika-app --all-targets --features sepika-app/gui,sepika-io/parquet --locked -- -D warnings`、`cargo fmt --all -- --check`、`cargo run -p xtask -- check-docs`、`mdbook build`、`git diff --check` も成功した。
追補前に `rustup update stable` を実行し、stable/clippy 1.99.1系が最新のままであることを確認した。CIの再実行は公開PRで確認する。

## 未実施・対象外

手動GUI操作・画面描画の目視、本控除専用のMCP受入、実験・実測によるValidation、外部資料による規準照合は未実施。
部分取付きとAttachedの重複算定、嵩上げ加算、偏心による応力変換、動的物理質量の重複除去は対象外。
全workspaceの回帰試験とCIの結果はこの記録のローカル検証とは別であり、CIが網羅的な構成を担当する。独立レビュー・最終headのCI・mergeは公開PRの最終ゲートで確認する。
