# 非線形解析の塑性率・NewRC・コンクリート履歴則（Issue #956）

確認日 2026-11-18。製品実装の確認基点は `81cbae55229d`。調査範囲は現行の増分解析と非線形立体時刻歴応答解析で使用する値・分岐。製品仕様は[プッシュオーバー解析](../../docs/calc_basis/05_構造解析/07_プッシュオーバー解析.md)、[一軸材料構成則](../../docs/nonlinear/01_一軸材料構成則.md)、[部材履歴則](../../docs/nonlinear/02_部材履歴則.md)を参照。[調査 #956](https://github.com/hrntsm/SEPIKA/issues/956)・[分離元 #697](https://github.com/hrntsm/SEPIKA/issues/697)・[外挿決定 #385](https://github.com/hrntsm/SEPIKA/issues/385)。

## 製品で値が使われる経路

| 対象 | 選択・生成・使用 | 入力で選べる範囲 |
| --- | --- | --- |
| 増分解析 | [アプリ](../../crates/sepika-app/src/app/actions/pushover.rs)・[外部連携](https://github.com/hrntsm/SEPIKA/blob/8ae832fa7b87dc61dfd03198a1a4bdbbb2eca0cf/crates/sepika-mcp/src/job/pushover.rs) → [compute_pushover](../../crates/sepika-job/src/compute.rs) → [pushover_analysis_recording](../../crates/sepika-solver/src/nonlinear/pushover/driver.rs) → [非線形要素の生成](../../crates/sepika-element/src/factory/mod.rs)（増分用、材料強度ベース）。[ステップ記録](../../crates/sepika-solver/src/nonlinear/pushover/step_record.rs)で[塑性率追跡](../../crates/sepika-solver/src/nonlinear/pushover/ductility.rs)と[曲げヒンジ記録](../../crates/sepika-solver/src/nonlinear/pushover/hinge.rs)を行う。塑性率・表示レベルで材料応力を更新したり、釣合い反復を打ち切ったりする経路はない。[機構前段ゲート](../../crates/sepika-solver/src/nonlinear/pushover/mechanism.rs)は降伏と終局を同じ条件で数える | [解析設定](../../crates/sepika-job/src/settings.rs)の3方式から選択できる。基点ひずみの個別値とμ閾値4は固定 |
| 基点と曲げヒンジ | ファイバー・MS の[プローブ](../../crates/sepika-element/src/frame/fiber/mod.rs)は端部2断面中の合成曲率の大きい方を選び、その全ファイバーの最大引張・圧縮ひずみを評価する。[壁](../../crates/sepika-element/src/wall/wall_element.rs)も壁柱ファイバーがあれば同じプローブへ委譲する。閾値の区分は主材料 `fc` の有無。初到達時の正の曲率を固定する。My以上の材端に同じ部材μを用い、曲率由来のμが0なら部材両端・二曲げ方向の最大絶対モーメントとMyの比を代用する | 曲率由来のμは基点未到達・プローブ非対応なら0。集中ばねは曲率プローブ非対応なのでモーメント比の経路に入る。壁柱プローブと耐震壁せん断ばねの降伏記録は別 |
| 非線形立体時刻歴 | [アプリ](../../crates/sepika-app/src/app/actions/time_history.rs)・[外部連携](https://github.com/hrntsm/SEPIKA/blob/8ae832fa7b87dc61dfd03198a1a4bdbbb2eca0cf/crates/sepika-mcp/src/job/time_history.rs) → [compute_time_history](../../crates/sepika-job/src/compute.rs) → [nonlinear_time_history_analysis](../../crates/sepika-solver/src/dynamic/timehistory/nonlinear.rs) → 同じ非線形要素生成（時刻歴用、公称強度ベース）。増分解析用の基点追跡・曲げヒンジ記録は通らない | 同じ部材でも時刻歴用の履歴指定が優先。未指定なら増分用の指定を継承し、自動なら時刻歴用の自動既定を解決 |
| NewRC と履歴 | 一般の梁要素は[部材方式](../../crates/sepika-element/src/factory/regime.rs)の自動判定で、非柱かつ剛床所属なら集中ばね、その他はファイバーになる。壁上下の弾性剛梁は[要素生成の先行分岐](../../crates/sepika-element/src/factory/mod.rs)で弾性梁として扱い、NewRCと塑性率プローブの対象外となる。ファイバー・MS線材のコンクリート領域と、増分解析でファイバー化するRC耐震壁柱が[共通の材料生成](../../crates/sepika-element/src/frame/fiber/mod.rs)から[NewRC](../../crates/sepika-material/src/newrc.rs)を使用する。[選択](../../crates/sepika-element/src/factory/springs.rs)は線材の自動値を増分で逆行型・時刻歴で原点指向型とし、壁柱の増分自動値は原点指向型。非線形立体時刻歴では、RC・SRC置換壁の非線形側柱接続を[入力チェック](../../crates/sepika-element/src/factory/input_check.rs)で理由付きで拒否する | 有効な明示指定は逆行型・原点指向型。Fcと材料Eは入力できる。解析に使うコンクリートシリンダー強度には主材料の設計基準強度Fcを代入し、拘束補正をしない。Fc≤60では断面に応じて解決したE（CFT充填部は単位体積重量とFcから算定、その他は材料E）を使い、Fc>60では固有Ecへ切り替える。係数・気乾単位容積質量2.4 t/m³は固定 |

線形解析や材端集中曲げばねのM–θ履歴にはNewRCの有理包絡線を用いない。耐震壁の面内せん断ばねのQ–δ履歴の自動既定は最大点指向型で、壁柱の一軸圧縮履歴とは別である。

## 原画像で確認した範囲

| 資料・箇所 | 照合結果 |
| --- | --- |
| 『2025年版 建築物の構造関係技術基準解説書』第2版第1刷（2026-02-11）、[書誌](https://ndlsearch.ndl.go.jp/books/R100000136-I1970868940118834340)、6.4 pp.423–424（6.4分冊のPDF 19–20頁）、7.3 pp.500–501、8.1 p.520 | 6.4はRC部材種別と脆性破壊の関係を扱う。p.423のb)は柱のせん断スパン比、p.424のd)は軸方向力と軸方向応力度比σ₀/Fcを説明する。7.3は部材性能に応じた曲率・ヒンジ領域長等の評価、RC・Sの安全限界変位、ヒンジ部材の部材角・塑性率の余裕を扱う。8.1は実状に応じた復元力特性の設定を求める。いずれの確認頁からも、現行のファイバー基点ひずみ、μ=4、材料の履歴自動既定は導けない |
| 日本建築学会『鋼構造塑性設計指針』第4版第1刷（2026-08-05）、[書誌](https://ci.nii.ac.jp/ncid/BD21038834)、5.4 p.83、図C5.4.1・式(C5.4.1)–(C5.4.2) | 梁端回転の骨格上の塑性変形倍率Rと履歴上の最大・累積塑性角を区別し、横座屈細長比と幅厚比により評価する。4θpは解析用繰返し載荷履歴の振幅の一つであり、SEPIKAの曲率比・モーメント比の閾値とは対応しない |
| 日本建築学会『鉄筋コンクリート構造計算規準・同解説』（2024年版）、[書誌](https://ci.nii.ac.jp/ncid/BD10678073)、5条 pp.57–58、表5.1・解説図5.1・式(解5.2) | 材料Eの表と、60 N/mm²で規準化するヤング係数推定式を確認。解説は1991年版式とNew RC式の比較に36 N/mm²を使い、ヤング係数の測定と初期接線との区別も示す。いずれも今回の一軸圧縮構成則を60で切替える根拠にはならない。引用文献1は総プロ報告書（1993.10） |
| 青山ほか、国土交通省建築研究所『鉄筋コンクリート造建築物の超軽量・超高層化技術の開発』建築研究報告No.139（2001）、3.1.2c pp.34–35、6.6.3 p.137・図6.10（各章PDF 10–11頁、7頁） | 3.1.2cは圧縮強度時ひずみに粗骨材の種類・量・最大寸法が影響することを示す。図3.1.20は圧縮強度36 MPa超のヤング係数推定を説明し、60 MPaを規準化の基準とする。6.6.3は柱・梁試験体の部材角と等価粘性減衰の関係を示し、履歴吸収が過大だと応答を過小評価する場合を指摘する。部材の変形・減衰に応じた履歴設定が必要であり、解析種別だけによる一軸材料則の固定選択は示していない。[第3章](https://www.kenken.go.jp/japanese/contents/publications/report/139/4.pdf#page=10)・[第6章](https://www.kenken.go.jp/japanese/contents/publications/report/139/7.pdf#page=7) |
| 崎野健治・孫玉平（1994）日本建築学会構造系論文集59(461)、pp.98・101–102、式(1)・(2)・(5)・(9)（PDF 4・7–8頁） | 式(1)は同形のSargin有理式で、初期剛性をAへ入れ、D>0とする。応力単位はkgf/cm²。式(2)のEcはMartinez式、式(5)のピークひずみ係数は0.52。角形鋼管拘束の回帰で得た式(9)はα=1.50、β=−1.68×10⁻³で側圧寄与を加える。帯筋には別の側圧係数を提案。拘束試験の回帰を任意強度・任意ひずみの無拘束材へ適用できるとは判断できない。[論文原画像](https://www.jstage.jst.go.jp/article/aijs/59/461/59_KJ00004099828/_pdf#page=4) |
| 土木研究所資料第3882号（2003）、1.3.6–1.4.1 pp.103–107、表1.5・式(1-2)・図1.21（PDF 109–113頁） | 総プロ報告書を引用して供試体と解析を比較した資料。圧縮有理式、εoの0.5243、Ec式の定数と一般骨材のk=1.0（高弾性1.2・低弾性0.9）、Dのα=1.50・β=−1.68×10⁻³を原画像で確認した。σBの単位は式が同形の崎野・孫p.98のkgf/cm²と公開柱マニュアルの工学単位系で照合。拘束効果を含む比較で、応力0.8σccまでの下降を終局比較点としている。表1.5では試験体の変位じん性率は2.85–9.29であり、約4.01の一体も載荷点変位で算定したものである。p.105の40 N/mm²の制限は比較相手の道路橋示方書式に付されたものであり、NewRC式の上限ではない。[研究資料](https://www.pwri.go.jp/jpn/results/db/doken_kankoubutu/doken_shiryou/files/doken_shiryou_3882_00.pdf#page=111) |
| 野口貴文・友澤史紀（1995）日本建築学会構造系論文集60(474)、pp.6–7、式(7)・(8)・表3・5（PDF 6–7頁） | 実用New RCヤング係数式では粗骨材と混和材を別々に補正する。60 MPaは代表値・規準化の基準。k=1相当は条件付きで、現行の工学単位式を換算したFc=60、γ=2.4で33,301 N/mm²に対し、論文式では33,500 MPa。別形式と係数差を確認し、初期接線の検証や全骨材のk=1の根拠には用いない。[論文原画像](https://www.jstage.jst.go.jp/article/aijs/60/474/60_KJ00004100171/_pdf/-char/ja#page=6) |

土研資料が引用する総プロの年度別報告書（1989.3–1993.3）と、RC規準が引用する総プロ報告書（1993.10）の当該式原画像は、指定資料群と公開検索からは取得できなかった。0.5243と工学単位のEc式の一致は土研資料の比較式と下記の公開方式説明まで。各定数の総プロ内での初出箇所・無拘束試験で妥当と確認された全強度域とひずみ域は保留する。2025年版の確認は表の関連頁の範囲で行った。

## 採用経緯と判断

- **基点ひずみ**。[導入コミット](https://github.com/hrntsm/SEPIKA/commit/91ba3837)の比較先の現行資料では、RCとSの値が一致する。公開FAQは終局耐力に十分に近づく程度の代表値という設定理由と、S柱で降伏ひずみの約5倍という目安を示す。SEPIKAは個々のファイバーの材料種でこの区分を切り替える設計ではなく、部材主材料のFcの有無で全ファイバーのひずみを比較する。現行の操作書は基点ひずみ方式を選んだ場合に数値を設定できるとし、旧公開系統の操作書にはS柱・RC柱・RC耐震壁ごとの初期値が示される。SEPIKAで選べるのは算定方式で、基点ひずみの値は固定。配筋・拘束・軸力・材種・断面・載荷条件に対する共通の耐力限界や安全性は確認できないため、現行の指標用既定と代替方式を維持する。[現行の公式柱計算書](https://kke-resp-team.github.io/resp-d-docs/calculation2/05_nonlinear_model/03_column/index.html)・[基点の公式FAQ](https://kke-resp-team.github.io/resp-d-docs/faq/calculation/q0005/index.html)・[現行の操作書](https://kke-resp-team.github.io/resp-d-docs/operation2/05_calculation_conditions/05_member_restoring_force_characteristics_calculation_conditions/index.html#塑性率基点歪みを設定)・[旧公開系統の操作書](https://resp-d-docs.gitlab.io/operation/docs/condition/)
- **固定値μ=4**。[導入コミット](https://github.com/hrntsm/SEPIKA/commit/91ba3837)で表示判定の定数4.0を設定し、当時のコードと照合記録にも原典要確認と明記した。公開FAQは塑性率1での柱ヒンジ描画を説明する。曲率の基準が方式で変わり、モーメント比も同じレベル分類に入り得るため、μ=4に共通する物理的終局変形は特定できない。表示の分類として維持し、設計上の終局限界に読み替えない。固定値の変更や個別入力の追加を正当化する根拠は得られなかった。[描画の公式FAQ](https://kke-resp-team.github.io/resp-d-docs/faq/output/q0006/index.html)
- **NewRC**。[導入コミット](https://github.com/hrntsm/SEPIKA/commit/f5bea3ff)は現行の正符号とFcの60区分を導入した。比較先の旧公開系統は無拘束の場合のDの強度項を正、現行の公式柱計算書は負とする。負符号は崎野・孫の実験に基づく提案式と土研の比較式に対応する。導入者が参照した版・時点と公開資料間の訂正経緯は未特定。比較先の両資料はFc>60 N/mm²またはユーザー定義コンクリートにバイリニア骨格を選ぶ。SEPIKAはFc>60で固有EcによるNewRCの暫定外挿を用い、材料の直接入力だけでは構成則を切り替えない。これは[#385](https://github.com/hrntsm/SEPIKA/issues/385)で整理した製品の採用条件であり、総プロの試験で実証された条件は未確認。Dの符号差は材料応力に現れるため、符号と適用条件の是正を[#1414](https://github.com/hrntsm/SEPIKA/issues/1414)で追跡する。Fc>60の応答、気乾単位容積質量・骨材補正、実強度と設計基準強度の対応は妥当性未確認。[旧公開系統の柱計算書](https://resp-d-docs.gitlab.io/calculation/docs/material-nonlinearmodel/#柱)・[現行の公式柱計算書](https://kke-resp-team.github.io/resp-d-docs/calculation2/05_nonlinear_model/03_column/index.html)
- **履歴則の自動既定**。[導入コミット](https://github.com/hrntsm/SEPIKA/commit/08c6ef4a)で静動別の自動選択が製品に入った。比較先の旧公開系統と現行公式版は、柱ファイバーのコンクリートで静的は逆行型、動的は原点指向型とし、SEPIKAの線材ファイバー・MSの選択と一致する。この一致は柱の方式選択について確認したもので、壁柱の扱い、総プロの実証条件や応答精度の裏付けは別途確認が必要。SEPIKAの現行は[PR #1346](https://github.com/hrntsm/SEPIKA/pull/1346)の原点指向型と既存の逆行型に限定される。片側圧縮の固定極値内を往復する場合、逆行型は包絡線、原点指向型は極値への割線を同じ経路で戻るため、どちらもこのループ単独では圧縮の履歴吸収を表現しない。原点指向型は圧縮側の残留ひずみ・ひび割れ閉鎖点移動を扱わない。応力再配分、反転後の層応答とエネルギーの誤差方向は一般には不定。単調が主体でも局所除荷は生じ、既定選択を一律の安全側とみなせない。実験や採用原典に基づく切替根拠が得られるまで既定と明示指定を維持する。[旧公開系統の柱計算書](https://resp-d-docs.gitlab.io/calculation/docs/material-nonlinearmodel/#柱)・[現行の公式柱計算書](https://kke-resp-team.github.io/resp-d-docs/calculation2/05_nonlinear_model/03_column/index.html)

## 有限例による差の確認

同じ有理式・A・ピークひずみで、Dの強度係数の符号だけを交換した代数比較。圧縮を正の大きさで示す。Fc=30には明示材料E=27,000 N/mm²、Fc=90には現行の固有Ecを使う。応力比は分子・分母の差から `1−(X−1)²/[1+(A−2)X+DX²]` として独立に計算した。

| Fc [N/mm²] | εo | Ec [N/mm²] | X=2での現行Dと応力 [N/mm²] | X=2での負符号比較Dと応力 [N/mm²] |
| --- | --- | --- | --- | --- |
| 30 | 0.002192704 | 27,000 | 2.013937、26.667635 | 0.986063、23.866435 |
| 90 | 0.002885761 | 38,120.150934 | 3.041811、82.249273 | −0.041811、214.537479（途中で特異化する式の値） |

Fc=30では圧縮ピークを越えた同じひずみで現行が約2.80 N/mm²高く、圧縮応力の保持を局所的に過大評価する。現行有理式の大ひずみ側は `Fc(1−1/D)` に漸近し、この例では15.10 N/mm²、Fc=90で60.41 N/mm²が残る。Fc=90の符号だけを換えると、分母がX≈1.207439でゼロになる。これは負符号の原典をそのまま高強度へ外挿できない具体例であり、原典の予測値として表の214.537479を使用しない。是正時は原典の対象強度・拘束と式の正則性、Fcと材料Eの境界、下降枝の打切りを併せて決める。架構の変位・応力・耐力に対する安全性をこの一軸比較から一般化しない。現行の両応力と計算用接線0を公開材料APIで実測し、表の独立値と10⁻⁹ N/mm²以内で一致した。

Fc=30の同じ試料を圧縮ピークまで載荷し、半分の圧縮ひずみ（X=0.5）に戻す。現行Dで逆行型は包絡線上の24.967122 N/mm²、原点指向型は割線上の15.000000 N/mm²となる。公開材料APIでピークを確定し、除荷応力が独立値と10⁻⁹ N/mm²以内で一致することを確認した。履歴全体の実験再現性や地震応答の大小はこの例では判定できない。

## 照合対象外

- [#847](https://github.com/hrntsm/SEPIKA/issues/847)で削除したManderモデル・未使用の履歴分岐（劣化武田型・Slip）。現行の非線形生成経路に存在しない。
- [#849](https://github.com/hrntsm/SEPIKA/issues/849)・[ADR 0080](../adr/0080-retire-reference-skeleton.md)で廃止した参考骨格計算。現行製品の骨格・曲率の参照として採用しない。
- [PR #1346](https://github.com/hrntsm/SEPIKA/pull/1346)で削除したKarsan–Jirsa型。現行の設定・生成経路から除外され、[旧保存指定の拒否テスト](../../crates/sepika-io/src/ovika.rs)に型名が残る。過去の試験結果は[当時の外挿検証](NewRC高強度コンクリート外挿_2026-09.md)を参照。

## 検証範囲

既存試験26件が成功した。材料のNewRC 10件、要素の履歴選択1件とFcの焦点9件、ソルバーのステップ記録3件と塑性率・曲げヒンジ3件を含む。製品文書はcheck-docsとmdBookビルドが成功した。

状態は🔶。確認範囲は原画像、コードの到達経路、有限例および上記の実行結果。実験Validation・実建物の応答・手動GUI・専門家の採用確認は未実施。
