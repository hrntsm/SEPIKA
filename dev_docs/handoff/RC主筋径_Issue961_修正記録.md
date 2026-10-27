# RC主筋径と長期許容応力度の境界修正

作成日: 2026-10-27

## 対象と根拠

[#961](https://github.com/hrntsm/SEPIKA/issues/961) の数値径28.6 mmに対する長期許容値の不整合を修正した。
[e-Govの建築基準法施行令](https://laws.e-gov.go.jp/law/325CO0000000338) 第90条表2は、異形鉄筋の径28 mm以下と28 mm超を区分する。
SD345・SD390の長期許容引張・圧縮応力度は、この表の径区分から215／195 N/mm²を独立期待値とした。
SD490の根拠は平成13年国土交通省告示第1024号第1第六号の高強度鉄筋表（長期主筋: 径28 mm以下はmin(F/1.5,215)、28 mm超はmin(F/1.5,195)）と第3第五号のF=490であり、同じ215／195 N/mm²となる。
告示の識別は[法令索引](https://hourei.ndl.go.jp/simple/detail?current=1&lawId=0000090316)による。認定に応じた個別指定値の入力機能は今回追加しない。
[公開寸法表](https://www.nipponsteel.com/product/construction/handbook/pdf/5-3.pdf) でD25の公称直径25.4 mm、D29の28.6 mmを確認した。

## 入力経路の調査

| 経路 | 径の意味・受理範囲 | 確認位置 |
| --- | --- | --- |
| 公開配筋型の直接構築 | `main_dia`はmmのf64。幾何検証では正の有限値と配置成立条件を検査し、JIS寸法表との一致は要求しない | `crates/sepika-core/src/section_shape/types.rs`、`crates/sepika-core/src/rc_rebar_geom.rs` |
| 断面編集の選択 | D10、13、16、19、22、25、29、32、35、38、41を選び、その整数を保持する | `crates/sepika-app/src/section_editor.rs::size_field`、`crates/sepika-core/src/material_grade.rs::REBAR_NOMINAL_SIZES` |
| STB標準配筋取込 | D/d接頭辞を除いた数値または数値文字列を正の有限値として受理。D29は29、数値28.6は28.6。JIS公称直径への変換はしない | `crates/sepika-io/src/stbridge/import/rebar.rs::parse_bar_dia` |

公開APIは型構築だけでは入力を拒否せず、`RcDesign::check`で配筋の幾何検証を行う。
材料許容値関数は正の有限な径を前提とする。任意径を標準呼び名へ丸める契約は追加していない。
STBの欠落・解決不能値は0となり、配筋の不足・不正として後段へ渡る。
STB規格全体の径属性の規範的意味を確定した記録ではなく、現行読込実装と受理範囲の調査である。

## 修正範囲と影響

共通許容値関数の長期境界を数値29以上から28超へ変更した。
RC矩形梁・矩形柱・円形柱、SRC梁・柱の共通関数利用経路に反映する。
SD345・SD390・SD490の数値径28超29未満で長期215を選ぶ危険側の材料値選択を解消する。
短期値、SR系・SD295系・USD685、せん断補強筋の許容値は変更しない。

主筋位置、円面積、鉄筋数量、保存径は変更しない。
GUI/STBの整数呼び名保持を公称直径へ一括変換すると、それらが変化し得るため本修正には含めない。
許容値関数は検定専用であり、解析剛性・質量・終局耐力の式は変更しない。
検定契約を43から44へ更新して旧保存検定の再使用を拒否する。解析契約65とモデル・結果保存スキーマは据え置く。
現行解析結果からの再検定は可能で、再保存だけでは旧検定を有効にしない。

## 検証の位置と限界

- `material_strength::tests::rebar_long_term_tension_follows_article_90_diameter_boundary_mm`: 令90条表2と告示第1024号の独立期待値で28前後、D25/D29の整数保持値と公称直径、3材種の長短期値を検証する。
- `rc::column::tests::public_column_check_uses_195_for_nominal_d29_diameter_mm`: 800×800 mm、Fc24、x=[6]/y=[4]、4面の中心65 mm、帯筋6.35 mm@100、N=-100 kN・Mz=1 kN·m、長期中央の公開入口を確認する。同じ幾何・需要で法令長期195のSD295Aと結果が一致することを検証する。N–M耐力・合成比そのものの独立正解を保証する試験ではない。
- STB既存試験 `test_standard_roundtrip_rc_rebar_non_integer` が数値径12.7/6.35の保持を、既存D22取込試験が呼び名整数保持を検証する。

全断面・全材料の耐力の独立正解、STB仕様上の呼び名と公称直径の使い分け、および既存整数保持による幾何・数量のJIS表との差は本修正の保証範囲外である。STB解釈の調査は分離元[#947](https://github.com/hrntsm/SEPIKA/issues/947)を参照する。
