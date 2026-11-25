# RC計算根拠の棚卸し

確認日 2026-11-22。RCの採用原典と維持する解析モデルは[ADR 0147](../adr/0147-rc-calculation-sources.md)による。この棚卸しでは資料の掲載範囲、現行方式との差、未確認の適用条件を記録した。算定の実装と保存形式の変更は各Issueで扱う。

## 採用原典

- **技術基準解説書** — 『2025年版 建築物の構造関係技術基準解説書』第2版第1刷。法令・告示は同書の掲載箇所と条項を対応づける。
- **RC規準** — 日本建築学会『鉄筋コンクリート構造計算規準・同解説』第10版（2024）。以下の頁は書籍の印刷頁。

## 算定項目と確認範囲

| 項目 | 指定原典で確認した箇所・範囲 | 現行方式との差・追跡先 |
| --- | --- | --- |
| 材料の初期弾性定数・重量 | RC規準5条・表5.1（p.8）のEc式と重量の注、7条・表7.1（p.10）。材料の種類・品質は3条で参照先が指定される | 重量表の範囲外、用途別の初期接線、実配筋・数量との関係は個別に確認する。[Ec](../../docs/calc_basis/03_断面性能/04_コンクリートのヤング係数_Ec.md) |
| 断面設計用ヤング係数比 | RC規準12条(3)・表12.1（p.17）のFc≤60の4区分と境界 | Fc>60のn=7は表の対象外。[#1869](https://github.com/hrntsm/SEPIKA/issues/1869) |
| 鉄筋の強度・用途 | 技術基準解説書9.2（鉄筋を含む基準強度、平成12年建設省告示第2464号。高強度鉄筋の許容応力度・材料強度、平成13年国土交通省告示第1024号）、RC規準4・6条と付12 | 認定別の指定値を名称から決める根拠は確認できない。[#991](https://github.com/hrntsm/SEPIKA/issues/991)。任意名称の強度割増、再生棒鋼、数量用線密度は個別課題。 |
| 梁・柱の許容曲げ、柱の構造規定 | RC規準12～14条（pp.17–19）、梁・柱の断面釣合い、14条3項と解説3（pp.150–153）の短期平均圧縮軸応力度 | 対象柱・例外を判別する入力と、現行の低圧縮側警告を是正する。[#1392](https://github.com/hrntsm/SEPIKA/issues/1392)。配筋集約・二軸・円形の全入力域への適用は別途確認。 |
| 梁・柱・接合部の許容せん断 | RC規準15条（pp.19–24）。柱長期の最大需要と必要時のαの選択は、同規準の解説p.177と付2 p.438も参照 | 部材の需要・材料と許容式・判定範囲の組合せ、接合部の形状・応力伝達は個別に確認する。[一次設計](../../docs/calc_basis/06_一次設計/02_RC_断面検定.md) |
| 梁の付着・定着 | RC規準16条1項(3)、式(16.1)～(16.6)（p.25）はせん断力や平均付着応力度など用途別の検討を示す。17条は定着 | 1991方式では地震時に現行の設計用せん断力を用いる。1999方式では多段筋等の代表化がある。方式名から適用可否を判断せず、長期・短期・大地震時の式と入力を照合する。[付着・定着](../../docs/calc_basis/06_一次設計/02_07_RC梁の付着_定着検定.md)・[丸鋼 #1802](https://github.com/hrntsm/SEPIKA/issues/1802) |
| 梁の長期たわみ、剛域・スラブ協力 | 技術基準解説書6.1の平成12年建設省告示第1459号、RC規準8条・付7 | 長期たわみの3点比較、両立を仮定するスラブ協力と接続条件の制限は、各方式の適用域を確認する。[梁の長期たわみ](../../docs/calc_basis/06_一次設計/02_10_RC梁の長期たわみ.md) |
| RC耐震壁の線形モデル・許容せん断 | RC規準8条、19条3～5項（pp.35–39）、技術基準解説書pp.708–712 | 縦横の小さい方の鉄筋比の共通使用、開口を含む軸・曲げ断面、側柱と開口補強の条件は要照合。[#652](https://github.com/hrntsm/SEPIKA/issues/652)・[#821](https://github.com/hrntsm/SEPIKA/issues/821) |
| RC耐震壁の部材種別 | 技術基準解説書6.4.4（p.425）。昭和55年建設省告示第1792号第4に関連する、壁の平均せん断応力度と側柱寸法の判定 | 2015年版の公開引用を2025年版の該当頁に置き換えた。側柱付き壁の照合と壁式構造そのものへの適用範囲を区別する。[部材ランク](../../docs/calc_basis/07_二次設計/03_部材ランク.md) |
| 梁・柱の略算終局耐力、曲げヒンジ | 技術基準解説書付録1-3.1 pp.698–706。梁・柱の曲げとせん断の略算、円形柱の等面積置換の例 | 略算の掲載を、引張軸力、非対称配筋、円形置換、ヒンジ閾値、需要の全域へ読み替えない。[#1712](https://github.com/hrntsm/SEPIKA/issues/1712)・[終局耐力](../../docs/calc_basis/08_部材終局耐力/01_RC_部材_略算終局耐力.md) |
| 梁・柱の終局せん断耐力 | 技術基準解説書pp.695–696の対象材料と材料強度、pp.699–705の式(付1.3-7)・(付1.3-16) | 荒川mean式を採用。普通コンクリートの矩形断面で、必要な確定内力と配筋を確認できる場合に限る。大臣認定の材料に同式を適用しない。せん断補強筋の基準強度に割増を適用せず、丸鋼295 N/mm²、異形鉄筋490 N/mm²を上限とする。[#1873](https://github.com/hrntsm/SEPIKA/issues/1873) |
| 大地震時の付着 | RC規準1・4条の対象材料、3条解説pp.53–54、16条1項(3)3) pp.25–27、16条解説pp.219、221、225–226 | 一段の矩形梁・柱で検討軸、対象材料と強度範囲、有効せい、曲げ降伏状態、配筋区分、付着検定断面からの長さ、補強条件を確認する。応力度照査には確認した実降伏強度または強度上昇を考慮した値を用いる。曲げ降伏しない場合は確認した長期・水平荷重時の鉄筋応力度を用い、通し筋は地震時曲げモーメント分布に応じて係数を選ぶ。通し筋とカットオフ筋を併用する場合は、両組の応力度と必要延長の各判定を確認する。多段は未算定。[#1827](https://github.com/hrntsm/SEPIKA/issues/1827)・[#1500](https://github.com/hrntsm/SEPIKA/issues/1500)・[#1502](https://github.com/hrntsm/SEPIKA/issues/1502) |
| 柱梁接合部の終局耐力 | 技術基準解説書付録1-3.1(6) pp.728–731、式(付1.3-47)～(付1.3-49)。形状、直交梁、有効幅の算定の掲載を確認 | コンクリート圧縮強度の入力対応、折曲げ定着、方向ごとの需要と適用条件は要照合。[接合部の終局耐力](../../docs/calc_basis/08_部材終局耐力/03_RC_柱梁接合部_終局耐力.md) |
| 壁の終局せん断耐力 | 技術基準解説書pp.715–717、式(付1.3-32)～(付1.3-34)。min／meanの係数、平均軸応力度、上下限、開口と横筋比の条件を確認 | 比0の反曲点断面での採用、軸応力度の0.4Fc上限の適用、側柱なし・開口の個別条件は要照合。[#1288](https://github.com/hrntsm/SEPIKA/issues/1288)・[壁の終局せん断](../../docs/calc_basis/08_部材終局耐力/08_03_RC壁の終局せん断耐力_荒川min_mean.md) |
| 梁の降伏時剛性低下率と骨格 | 技術基準解説書pp.697–698、式(付1.3-3)・(付1.3-4)、RC規準8条解説pp.79–84（式(解8.20)～(解8.23)とT形補正） | 軸力を見込まない矩形梁に短スパン式と基本式を用い、曲げひび割れ点は鉄筋を考慮した断面係数で求める。降伏モーメントは両スパン区分とも引張鉄筋比0.4～2.8%で、引張・圧縮鉄筋の重心間距離を用いる(解8.20)で求める。集中ばねの対象と採用条件はADR 0147による。[#1874](https://github.com/hrntsm/SEPIKA/issues/1874)[#941の既存照合](RC梁αy_Issue941_採用式照合.md) |
| 壁の参考骨格 | 技術基準解説書p.712に軸応力度を含む(付1.3-26)と、採用した(付1.3-27)がある。p.714とRC規準p.88には終局時の剛性低下率式が掲載されている | [#1870の照合記録](RC壁の参考骨格Qc_Issue1870_照合記録.md)を参照。終局時剛性低下率式の壁筋比は、RC規準が引用する出典を設計判断時に確認した結果に基づき、実壁厚を用いた横筋比とする。[壁の参考骨格](../../docs/nonlinear/05_RC_部材_非線形復元力特性.md#耐震壁せん断非線形トリリニア) |
| ファイバー・履歴則・共通数値解法 | 技術基準解説書p.714に壁板のみのファイバー断面の例がある | ファイバー要素と履歴則は維持。NewRCの単調圧縮包絡線の下降枝等は個別に扱う。共通の質点系縮約や数値解法の文献をRC規準式の根拠に用いない。[#1414](https://github.com/hrntsm/SEPIKA/issues/1414)・[非線形モデル](../../docs/nonlinear/README.md) |
| ねじり、数量積算 | RC規準22条1項・解説（p.44、pp.414–419）はねじりを扱い、付12に鉄筋の公称寸法表がある | RCねじりは[#1548](https://github.com/hrntsm/SEPIKA/issues/1548)。概算鉄筋比、定着・継手の推定値、任意径の単位質量は工学的適用条件の照合が残る。[数量積算](../../docs/calc_basis/10_数量積算/01_数量積算の算定式.md) |

一段配筋の矩形RC梁の公開検定では、両端降伏のD22通し筋に実降伏強度345 N/mm²を確認した場合の需要は0.690 N/mm²、確認値379.5 N/mm²の場合は0.759 N/mm²となる。カットオフ筋は有効せい500 mm、計算上不要となる位置まで200 mm、付着長さ400 mmとし、応力度未算定と必要延長700 mmに対する不適合が同時に保持されることを確認した。

矩形RC架構の非線形時刻歴回帰は、標準4-D25・かぶり50 mmで、波形、刻み0.05秒、収束条件を維持して120秒まで完走した。かぶり40 mmでは2.65秒に全体釣合い、55 mmでは0.55秒に要素の断面釣合いが収束しなかった。載荷初期の停止条件は[#1875](https://github.com/hrntsm/SEPIKA/issues/1875)で追跡する。

### 矩形RC梁・柱の終局せん断 [#1873](https://github.com/hrntsm/SEPIKA/issues/1873)

無荷重の内法を対象に、剛体アーム長と異なる危険断面フェース位置の曲げを独立した釣合いで照合した。斜交した線材の単軸応答は、弱軸の曲げ・せん断が厳密にゼロのまま保持されることも確認した。せい600 mm・幅400 mmの支持柱に幅側から付く梁では、名目距離300 mmではなく実柱面の200 mmを用いた。梁芯が節点から下に50 mm偏心した接合では、下の柱から見た実梁面は節点から350 mmであることを断面内力から照合した。公開検定はFc24・SD345・主筋3-D19・補強筋2-D10@100の矩形断面で、設定した材料強度割増1.1を使わず計算することを確認した。梁250×350 mmは有効せい290.5 mm、せん断スパン比2、軸力ゼロで耐力164.779006 kN、余裕度1.647790。柱400×600 mmは有効せい540.5 mm、同じせん断スパン比、圧縮軸力範囲480～600 kNの下限から軸応力度2 N/mm²で耐力422.333712 kN、余裕度4.223337。いずれも確定総せん断力は100 kNとした。別途、材軸長1500 mm、幅400 mmで両端を支持する柱に取り付く梁で、実内法1100 mmと従来の控除1000 mmずつの不成立を実増分で再現した。実柱面間の確定総せん断力10 kN、最大曲げ5.5 kN·m、軸力0から耐力163.789084 kN、余裕度16.378908を照合し、曲げ・付着は従来の前提どおり未算定となることを確認した。

非引張柱で帯筋比0.012・圧縮軸応力度0.4Fcの上限、二軸同時作用と軸別結果の区別、断面力確認なし・引張柱・軽量コンクリート・多段配筋柱・補強筋強度不一致の未算定理由を照合する。部材荷重下での内法の極値回復と、材軸ねじりとの組合せは未確認。カットオフ筋、部分的な床板支持、一本部材グループを理由付き未算定とする公開入口も確認する。ねじりは[#1888](https://github.com/hrntsm/SEPIKA/issues/1888)、一本部材グループの照査対応は[#1889](https://github.com/hrntsm/SEPIKA/issues/1889)で追跡する。

## GitHub Issue

対象外として閉じたのは、旧版を対象とする柱の短期軸応力調査 [#1008](https://github.com/hrntsm/SEPIKA/issues/1008) と、指定外資料を前提とする一段配筋終局付着の追加調査 [#1805](https://github.com/hrntsm/SEPIKA/issues/1805)。現行コードの是正はそれぞれ [#1392](https://github.com/hrntsm/SEPIKA/issues/1392) と [#1827](https://github.com/hrntsm/SEPIKA/issues/1827) に引き継いだ。あばら筋のないRC梁に対する靭性指針式固有の適用条件の調査 [#1861](https://github.com/hrntsm/SEPIKA/issues/1861) も対象外として閉じ、再現条件と観測結果をIssueに残した。

ねじり照査はRCの [#1548](https://github.com/hrntsm/SEPIKA/issues/1548)、SRCの [#1867](https://github.com/hrntsm/SEPIKA/issues/1867)、CFTの [#1868](https://github.com/hrntsm/SEPIKA/issues/1868) に分けた。現行式の見直しは設計用ヤング係数比 [#1869](https://github.com/hrntsm/SEPIKA/issues/1869) と壁の参考骨格のひび割れ耐力 [#1870](https://github.com/hrntsm/SEPIKA/issues/1870) を追加した。両原典で追加確認できる範囲や、参照変更だけで再現が消えると確認できない不具合は継続する。

### オープンIssue全件の再確認

再確認時にオープンだった118件は、作成日を重複しない期間に区切った取得結果と、作成日の昇順・降順をそれぞれ100件まで取得して合わせた結果が一致した。タイトルと本文の目的・完了条件から、RCの技術的根拠や共通機能に関係するものを確認した。以下の番号はこの時点の確認対象。ここから新規に起票した課題は集計外。

- RCの算定・表示・数量積算への関係を確認した43件。#652、#821、#991、#1167、#1168、#1268、#1276、#1288、#1290、#1306、#1392、#1414、#1460、#1488、#1498、#1500、#1502、#1532、#1548、#1582、#1585、#1712、#1721、#1770、#1783、#1797、#1802、#1804、#1815、#1817、#1822、#1826、#1827、#1830、#1837、#1845、#1846、#1851、#1860、#1864、#1866、#1869、#1870。
- 他構造種別や開発基盤等を主目的とする75件。#723、#752、#764、#935、#950、#951、#1001、#1007、#1124、#1166、#1261、#1365、#1371、#1380、#1407、#1418、#1449、#1451、#1452、#1453、#1484、#1487、#1489、#1491、#1495、#1505、#1509、#1514、#1518、#1526、#1541、#1558、#1568、#1571、#1591、#1598、#1599、#1602、#1603、#1612、#1618、#1620、#1622、#1623、#1626、#1628、#1634、#1642、#1649、#1660、#1664、#1666、#1667、#1671、#1672、#1691、#1694、#1695、#1696、#1700、#1732、#1733、#1735、#1736、#1758、#1766、#1769、#1778、#1785、#1791、#1818、#1847、#1863、#1867、#1868。

- 靭性指針式の選択と計算経路を廃止する作業を [#1871](https://github.com/hrntsm/SEPIKA/issues/1871) に記録した。段別付着 [#1500](https://github.com/hrntsm/SEPIKA/issues/1500) と一段配筋の付着信頼強度 [#1827](https://github.com/hrntsm/SEPIKA/issues/1827) は指定の二冊で根拠を確かめる範囲に絞った。
- [#1815](https://github.com/hrntsm/SEPIKA/issues/1815) は、靭性指針式の廃止に伴い、再現条件だった終局せん断耐力ゼロの経路がなくなることを確認して閉じた。荒川mean式の需要ゼロと未算定の扱いは回帰テストで確認した。
- [#1582](https://github.com/hrntsm/SEPIKA/issues/1582) は終局設計用せん断力に絞り、地震方向別の終局付着需要を [#1872](https://github.com/hrntsm/SEPIKA/issues/1872) に分けた。
- RCに用いるD値法 [#1167](https://github.com/hrntsm/SEPIKA/issues/1167)、非線形解析への圧縮包絡線の追加 [#1168](https://github.com/hrntsm/SEPIKA/issues/1168)、圧縮包絡線の下降枝 [#1414](https://github.com/hrntsm/SEPIKA/issues/1414) は、指定原典で確認できる条件を着手時に特定する。S造合成梁 [#1306](https://github.com/hrntsm/SEPIKA/issues/1306) に記載されたRC協力幅の参照先は2024年版RC規準に更新した。

### 棚卸しで確認したIssue

GitHubのオープンIssueをRC、鉄筋コンクリート、付着、せん断、配筋、靭性指針等で検索し、RCの計算、入力・結果、または関連する共同機能を扱うものを確認した。以下は検索で特定した課題と今回追加した追跡先である。過去の閉鎖済み調査は、上の照合表から辿れるものを記録した。

### 材料・断面・重量・数量

- [#991 名称だけで指定したUSD685主筋の許容応力度を未算定として扱う](https://github.com/hrntsm/SEPIKA/issues/991)
- [#1585 鉄筋数量で任意の数値径に使う単位質量を確認する](https://github.com/hrntsm/SEPIKA/issues/1585)
- [#1721 再生棒鋼の主筋を入力・計算できる条件を確認する](https://github.com/hrntsm/SEPIKA/issues/1721)
- [#1770 数量積算の鉄筋方向別CSVで床板ごとの床筋を特定できるようにする](https://github.com/hrntsm/SEPIKA/issues/1770)
- [#1797 自動認識されない主筋の材料強度割増の適用条件を確認する](https://github.com/hrntsm/SEPIKA/issues/1797)
- [#1869 RC断面検定でFcが60 N/mm²を超えるとき設計用ヤング係数比を未算定にする](https://github.com/hrntsm/SEPIKA/issues/1869)

### 許容応力度・需要・付着

- [#1008 RC柱の短期軸応力度警告の不等号と適用条件を確認する](https://github.com/hrntsm/SEPIKA/issues/1008)（対象外でクローズ）
- [#1392 RC柱の短期軸応力度警告を対象条件と設計判断に対応させる](https://github.com/hrntsm/SEPIKA/issues/1392)
- [#1548 RC部材の許容ねじり照査に適用する規準と条件を確定する](https://github.com/hrntsm/SEPIKA/issues/1548)
- [#1802 丸鋼を使うRC梁で一次設計の付着割裂式の適用条件を確認する](https://github.com/hrntsm/SEPIKA/issues/1802)
- [#1804 RC柱の通常の地震時せん断検定で重力参照内力の不足を明示する](https://github.com/hrntsm/SEPIKA/issues/1804)
- [#1817 RC柱の設計用せん断力で梁降伏側の候補を係数適用後に比較する](https://github.com/hrntsm/SEPIKA/issues/1817)
- [#1845 片側の材端フェースが部材中央を越えると通常検定の材端位置がずれる](https://github.com/hrntsm/SEPIKA/issues/1845)
- [#1846 接続大梁の断面回転をRC柱の通常検定の材端位置に反映する](https://github.com/hrntsm/SEPIKA/issues/1846)
- [#1851 短期地震時のRC梁の設計用せん断力に両端の降伏曲げを反映する](https://github.com/hrntsm/SEPIKA/issues/1851)
- [#1860 RC柱の純軸力で検定比を表せないときも軸力超過を表示する](https://github.com/hrntsm/SEPIKA/issues/1860)
- [#1864 RC柱の長期単独荷重ケースの検定で非有限の軸力を適合と判定しない](https://github.com/hrntsm/SEPIKA/issues/1864)
- [#1866 RC柱の短期せん断検定で許容値と検定比の数値範囲を確認する](https://github.com/hrntsm/SEPIKA/issues/1866)

### 終局耐力・部材種別

- [#1873 RC梁・柱の終局せん断耐力に荒川mean式を用いる](https://github.com/hrntsm/SEPIKA/issues/1873)

- [#1268 矩形RC柱のランク判定用せん断応力度比の単位を整合させる](https://github.com/hrntsm/SEPIKA/issues/1268)
- [#1288 RC壁のせん断スパン比0に対する受理条件を整合させる](https://github.com/hrntsm/SEPIKA/issues/1288)
- [#1488 RC柱の軸終局耐力と終局検定の軸力判定の関係を確認する](https://github.com/hrntsm/SEPIKA/issues/1488)
- [#1500 多段配筋を持つRC梁の終局付着評価の適用範囲を確認する](https://github.com/hrntsm/SEPIKA/issues/1500)
- [#1502 短スパンRC梁などで必要となる終局付着の追加照査を確認する](https://github.com/hrntsm/SEPIKA/issues/1502)
- [#1532 ファイバー・マルチスプリングのRC梁・柱で終局検定に用いるフェースを確認する](https://github.com/hrntsm/SEPIKA/issues/1532)
- [#1582 RC梁の終局設計用せん断力に地震方向別の両端曲げ耐力を反映する](https://github.com/hrntsm/SEPIKA/issues/1582)
- [#1871 RC梁・柱の終局検定から靭性指針式の選択を廃止する](https://github.com/hrntsm/SEPIKA/issues/1871)
- [#1872 RC梁の大地震時付着照査に地震方向別の鉄筋応力度を反映する](https://github.com/hrntsm/SEPIKA/issues/1872)
- [#1712 RC梁の曲げヒンジ表示に使う略算の適用条件を確認する](https://github.com/hrntsm/SEPIKA/issues/1712)
- [#1805 異形鉄筋が1段のRC梁・柱で終局付着の強度式と適用条件を照合する](https://github.com/hrntsm/SEPIKA/issues/1805)（対象外でクローズ）
- [#1815 RC柱の終局二軸せん断で需要と耐力がともにゼロの方向を扱う](https://github.com/hrntsm/SEPIKA/issues/1815)
- [#1826 壁上下のRC剛梁の自動部材種別でせん断補強筋の材料区分を確認する](https://github.com/hrntsm/SEPIKA/issues/1826)
- [#1827 一段配筋のRC梁・柱に大地震時の付着応力度照査を適用する](https://github.com/hrntsm/SEPIKA/issues/1827)
- [#1837 壁上下のRC剛梁の自動部材種別でせん断補強筋の降伏強度を確認する](https://github.com/hrntsm/SEPIKA/issues/1837)
- [#1861 あばら筋がないRC梁への靭性指針式の適用条件を確認する](https://github.com/hrntsm/SEPIKA/issues/1861)（対象外でクローズ）

### 解析・モデル・表示

- [#652 RC壁の線形換算剛性と方向別配筋入力を照合する](https://github.com/hrntsm/SEPIKA/issues/652)
- [#821 RC耐震壁の軸・曲げ断面への開口評価と適用範囲を確認する](https://github.com/hrntsm/SEPIKA/issues/821)
- [#1168 コンクリートの構成則を画面で選択して非線形解析に適用できるようにする](https://github.com/hrntsm/SEPIKA/issues/1168)
- [#1414 NewRCの圧縮応力を下降枝の参照式と適用条件に整合させる](https://github.com/hrntsm/SEPIKA/issues/1414)
- [#1498 RC片持ち梁の自由端に残る微小な曲げの扱いを確認する](https://github.com/hrntsm/SEPIKA/issues/1498)
- [#1783 「壁の非線形モデル」で面内せん断ばねの標準型の呼び方を揃える](https://github.com/hrntsm/SEPIKA/issues/1783)
- [#1822 再表示時に計算する終局検定の変更で保存済みの通常検定を失効させない](https://github.com/hrntsm/SEPIKA/issues/1822)
- [#1830 時刻歴の設計用せん断力に初期載荷と同じ荷重条件の重力内力を使う](https://github.com/hrntsm/SEPIKA/issues/1830)
- [#1870 RC壁の参考骨格に使うせん断ひび割れ耐力を指定原典に合わせる](https://github.com/hrntsm/SEPIKA/issues/1870)

### 分離したSRC・CFTのねじり照査

- [#1867 SRC柱・梁の許容ねじり照査の算定条件を確定する](https://github.com/hrntsm/SEPIKA/issues/1867)
- [#1868 純充填型CFT柱の許容ねじり照査の算定条件を確定する](https://github.com/hrntsm/SEPIKA/issues/1868)

## 変更後の照合

- RC梁の降伏時剛性低下率は、n=10、pt=1%、d/D=0.9、軸力ゼロで a/D が1.0、1.5、2.0、5.0のとき0.061074、0.125469、0.23733、0.34182。原典の短スパン式と基本式を独立算術で評価した。中間の境界は基本式を採用し、適用範囲外へ数値を外挿しない。長期軸力の固定と軸力別の骨格更新は、設計判断時に検討した。今回の接続は軸力を見込まないことを明示した梁に限る。

- 終局せん断は、梁の幅400 mm・有効せい539 mm、柱の幅600 mm・有効せい639 mmの独立計算を用意した。どちらもFc24、補強筋の降伏強度345 N/mm²、M/(Qd)=2。柱の最小圧縮軸力840 kNによる平均圧縮応力度は2 N/mm²。期待する終局せん断耐力は梁421.174749 kN、柱680.049570 kN。部材荷重がない場合のフェース内力を使用する。上下で配筋が異なる場合は、両側の引張状態に対する値の小さい方を使う。
- 通し筋のみの内法長さ2000 mm、有効せい500 mm、設計用鉄筋応力度300 N/mm²、呼び径20、応力度係数1では設計用付着応力度は1.0 N/mm²。同じ鉄筋で計算上不要となる位置までの距離500 mmのカットオフ筋を併用すると、通し筋は1.5 N/mm²。付着長さ900 mm・不要位置まで400 mmのカットオフ筋は3.75 N/mm²、899 mmは必要な付着長さを満たさない。設計タブで配筋区分や必要寸法を確定できないときは未算定。
- 無開口耐震壁の初期せん断ひび割れ耐力は、付録式(付1.3-27)をSI換算した。荷重・変形の独立値と、Qc単体・骨格全体で異なる強度制限は[#1870の照合記録](RC壁の参考骨格Qc_Issue1870_照合記録.md#独立値と境界)を参照。
- 許容応力度計算でヤング係数比を必要とする経路は、RC規準の表が扱うFc60 N/mm²以下の範囲を使用する。USD685系主筋は、名称と降伏強度の設定から個別認定の許容値を確定できない。
- 上記の数値例は式の照合用。実建物の保証設計、実験に対する予測精度、合否への影響は条件ごとの確認を要する。

## 文書の検索結果

追跡対象のMarkdownについて、ファイル名と本文を「RC」「鉄筋コンクリート」「荒川」「靭性・靱性指針」「NewRC」「USD685」などで検索した。件数は該当ファイル数、各ファイルの行番号はこの棚卸し時点での最初の一致箇所。UI・保存仕様・単なる言及、SRC等の併記、過去の検証記録も候補に含む。計算の採用根拠は上の照合表を参照する。

### 製品文書（106件）

- [docs/SUMMARY.md](<../../docs/SUMMARY.md>)（28行、先頭68行目）
- [docs/calc_basis/01_荷重/01_地震力.md](<../../docs/calc_basis/01_荷重/01_地震力.md>)（2行、先頭199行目）
- [docs/calc_basis/01_荷重/03_床荷重の分配.md](<../../docs/calc_basis/01_荷重/03_床荷重の分配.md>)（2行、先頭441行目）
- [docs/calc_basis/01_荷重/04_荷重組合せ.md](<../../docs/calc_basis/01_荷重/04_荷重組合せ.md>)（1行、先頭72行目）
- [docs/calc_basis/01_荷重/05_地震用重量_層データの生成.md](<../../docs/calc_basis/01_荷重/05_地震用重量_層データの生成.md>)（18行、先頭86行目）
- [docs/calc_basis/01_荷重/06_自重の長期応力解析への接続.md](<../../docs/calc_basis/01_荷重/06_自重の長期応力解析への接続.md>)（4行、先頭7行目）
- [docs/calc_basis/01_荷重/08_床の断面と自重.md](<../../docs/calc_basis/01_荷重/08_床の断面と自重.md>)（3行、先頭15行目）
- [docs/calc_basis/01_荷重/09_壁の断面と自重.md](<../../docs/calc_basis/01_荷重/09_壁の断面と自重.md>)（2行、先頭20行目）
- [docs/calc_basis/02_材料/03_材料強度.md](<../../docs/calc_basis/02_材料/03_材料強度.md>)（3行、先頭16行目）
- [docs/calc_basis/02_材料/04_標準材料一覧.md](<../../docs/calc_basis/02_材料/04_標準材料一覧.md>)（10行、先頭7行目）
- [docs/calc_basis/02_材料/05_構造種別の判定.md](<../../docs/calc_basis/02_材料/05_構造種別の判定.md>)（11行、先頭1行目）
- [docs/calc_basis/02_材料/README.md](<../../docs/calc_basis/02_材料/README.md>)（1行、先頭9行目）
- [docs/calc_basis/03_断面性能/01_主要定数.md](<../../docs/calc_basis/03_断面性能/01_主要定数.md>)（1行、先頭8行目）
- [docs/calc_basis/03_断面性能/02_断面諸量.md](<../../docs/calc_basis/03_断面性能/02_断面諸量.md>)（4行、先頭85行目）
- [docs/calc_basis/03_断面性能/03_せん断有効断面積_As.md](<../../docs/calc_basis/03_断面性能/03_せん断有効断面積_As.md>)（4行、先頭4行目）
- [docs/calc_basis/03_断面性能/04_コンクリートのヤング係数_Ec.md](<../../docs/calc_basis/03_断面性能/04_コンクリートのヤング係数_Ec.md>)（6行、先頭9行目）
- [docs/calc_basis/03_断面性能/05_耐震壁のせん断形状係数_κ.md](<../../docs/calc_basis/03_断面性能/05_耐震壁のせん断形状係数_κ.md>)（1行、先頭43行目）
- [docs/calc_basis/03_断面性能/06_SRC_CFT_の等価断面性能.md](<../../docs/calc_basis/03_断面性能/06_SRC_CFT_の等価断面性能.md>)（12行、先頭29行目）
- [docs/calc_basis/04_要素剛性/01_ティモシェンコ梁要素.md](<../../docs/calc_basis/04_要素剛性/01_ティモシェンコ梁要素.md>)（16行、先頭68行目）
- [docs/calc_basis/04_要素剛性/03_部材荷重.md](<../../docs/calc_basis/04_要素剛性/03_部材荷重.md>)（3行、先頭112行目）
- [docs/calc_basis/04_要素剛性/05_壁エレメントモデル.md](<../../docs/calc_basis/04_要素剛性/05_壁エレメントモデル.md>)（21行、先頭9行目）
- [docs/calc_basis/04_要素剛性/06_パネルゾーン.md](<../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md>)（3行、先頭21行目）
- [docs/calc_basis/04_要素剛性/README.md](<../../docs/calc_basis/04_要素剛性/README.md>)（2行、先頭5行目）
- [docs/calc_basis/05_構造解析/05_時刻歴応答解析.md](<../../docs/calc_basis/05_構造解析/05_時刻歴応答解析.md>)（3行、先頭165行目）
- [docs/calc_basis/05_構造解析/07_プッシュオーバー解析.md](<../../docs/calc_basis/05_構造解析/07_プッシュオーバー解析.md>)（20行、先頭11行目）
- [docs/calc_basis/05_構造解析/09_質点系解析.md](<../../docs/calc_basis/05_構造解析/09_質点系解析.md>)（5行、先頭330行目）
- [docs/calc_basis/06_一次設計/01_01_コンクリートの許容応力度.md](<../../docs/calc_basis/06_一次設計/01_01_コンクリートの許容応力度.md>)（2行、先頭74行目）
- [docs/calc_basis/06_一次設計/01_02_鉄筋の許容応力度.md](<../../docs/calc_basis/06_一次設計/01_02_鉄筋の許容応力度.md>)（15行、先頭3行目）
- [docs/calc_basis/06_一次設計/01_材料の許容応力度.md](<../../docs/calc_basis/06_一次設計/01_材料の許容応力度.md>)（2行、先頭4行目）
- [docs/calc_basis/06_一次設計/02_01_RC梁の許容曲げ.md](<../../docs/calc_basis/06_一次設計/02_01_RC梁の許容曲げ.md>)（11行、先頭1行目）
- [docs/calc_basis/06_一次設計/02_02_RC梁のせん断検定.md](<../../docs/calc_basis/06_一次設計/02_02_RC梁のせん断検定.md>)（9行、先頭1行目）
- [docs/calc_basis/06_一次設計/02_03_RC柱の軸力_二軸曲げ検定.md](<../../docs/calc_basis/06_一次設計/02_03_RC柱の軸力_二軸曲げ検定.md>)（12行、先頭1行目）
- [docs/calc_basis/06_一次設計/02_04_RC柱のせん断検定.md](<../../docs/calc_basis/06_一次設計/02_04_RC柱のせん断検定.md>)（15行、先頭1行目）
- [docs/calc_basis/06_一次設計/02_05_RC梁_柱の許容せん断の共通式.md](<../../docs/calc_basis/06_一次設計/02_05_RC梁_柱の許容せん断の共通式.md>)（12行、先頭1行目）
- [docs/calc_basis/06_一次設計/02_06_RC柱梁接合部のせん断検定.md](<../../docs/calc_basis/06_一次設計/02_06_RC柱梁接合部のせん断検定.md>)（6行、先頭1行目）
- [docs/calc_basis/06_一次設計/02_07_RC梁の付着_定着検定.md](<../../docs/calc_basis/06_一次設計/02_07_RC梁の付着_定着検定.md>)（9行、先頭1行目）
- [docs/calc_basis/06_一次設計/02_08_RC梁の構造規定.md](<../../docs/calc_basis/06_一次設計/02_08_RC梁の構造規定.md>)（5行、先頭1行目）
- [docs/calc_basis/06_一次設計/02_09_RC柱の構造規定.md](<../../docs/calc_basis/06_一次設計/02_09_RC柱の構造規定.md>)（6行、先頭1行目）
- [docs/calc_basis/06_一次設計/02_10_RC梁の長期たわみ.md](<../../docs/calc_basis/06_一次設計/02_10_RC梁の長期たわみ.md>)（5行、先頭1行目）
- [docs/calc_basis/06_一次設計/02_11_RC耐震壁の許容せん断検定.md](<../../docs/calc_basis/06_一次設計/02_11_RC耐震壁の許容せん断検定.md>)（12行、先頭1行目）
- [docs/calc_basis/06_一次設計/02_RC_断面検定.md](<../../docs/calc_basis/06_一次設計/02_RC_断面検定.md>)（18行、先頭1行目）
- [docs/calc_basis/06_一次設計/03_09_鋼大梁の長期たわみ.md](<../../docs/calc_basis/06_一次設計/03_09_鋼大梁の長期たわみ.md>)（3行、先頭3行目）
- [docs/calc_basis/06_一次設計/03_鋼構造_断面検定.md](<../../docs/calc_basis/06_一次設計/03_鋼構造_断面検定.md>)（1行、先頭13行目）
- [docs/calc_basis/06_一次設計/04_01_SRCの部材検定.md](<../../docs/calc_basis/06_一次設計/04_01_SRCの部材検定.md>)（19行、先頭3行目）
- [docs/calc_basis/06_一次設計/04_02_SRC梁_柱の許容曲げ検定.md](<../../docs/calc_basis/06_一次設計/04_02_SRC梁_柱の許容曲げ検定.md>)（6行、先頭3行目）
- [docs/calc_basis/06_一次設計/04_03_SRC梁_柱の許容せん断検定.md](<../../docs/calc_basis/06_一次設計/04_03_SRC梁_柱の許容せん断検定.md>)（15行、先頭39行目）
- [docs/calc_basis/06_一次設計/04_04_SRC柱梁接合部のせん断検定.md](<../../docs/calc_basis/06_一次設計/04_04_SRC柱梁接合部のせん断検定.md>)（7行、先頭3行目）
- [docs/calc_basis/06_一次設計/04_SRC_CFT_断面検定.md](<../../docs/calc_basis/06_一次設計/04_SRC_CFT_断面検定.md>)（3行、先頭22行目）
- [docs/calc_basis/06_一次設計/05_01_鋼小梁の曲げ_たわみ検定.md](<../../docs/calc_basis/06_一次設計/05_01_鋼小梁の曲げ_たわみ検定.md>)（2行、先頭155行目）
- [docs/calc_basis/06_一次設計/05_04_独立鋼間柱の断面検定.md](<../../docs/calc_basis/06_一次設計/05_04_独立鋼間柱の断面検定.md>)（1行、先頭9行目）
- [docs/calc_basis/06_一次設計/07_01_S_CFT柱梁接合部の仕口パネル検定.md](<../../docs/calc_basis/06_一次設計/07_01_S_CFT柱梁接合部の仕口パネル検定.md>)（2行、先頭11行目）
- [docs/calc_basis/06_一次設計/README.md](<../../docs/calc_basis/06_一次設計/README.md>)（13行、先頭3行目）
- [docs/calc_basis/07_二次設計/01_保有水平耐力の判定.md](<../../docs/calc_basis/07_二次設計/01_保有水平耐力の判定.md>)（9行、先頭44行目）
- [docs/calc_basis/07_二次設計/02_構造特性係数_Ds.md](<../../docs/calc_basis/07_二次設計/02_構造特性係数_Ds.md>)（9行、先頭6行目）
- [docs/calc_basis/07_二次設計/03_部材ランク.md](<../../docs/calc_basis/07_二次設計/03_部材ランク.md>)（14行、先頭3行目）
- [docs/calc_basis/07_二次設計/04_形状係数_Fes.md](<../../docs/calc_basis/07_二次設計/04_形状係数_Fes.md>)（2行、先頭161行目）
- [docs/calc_basis/07_二次設計/README.md](<../../docs/calc_basis/07_二次設計/README.md>)（1行、先頭39行目）
- [docs/calc_basis/08_部材終局耐力/01_01_RC梁の曲げ終局耐力と曲げ終局時せん断力.md](<../../docs/calc_basis/08_部材終局耐力/01_01_RC梁の曲げ終局耐力と曲げ終局時せん断力.md>)（5行、先頭1行目）
- [docs/calc_basis/08_部材終局耐力/01_02_RC柱の軸力を考慮した終局曲げ耐力.md](<../../docs/calc_basis/08_部材終局耐力/01_02_RC柱の軸力を考慮した終局曲げ耐力.md>)（4行、先頭1行目）
- [docs/calc_basis/08_部材終局耐力/01_03_RC梁_柱の終局せん断耐力_荒川mean式系.md](<../../docs/calc_basis/08_部材終局耐力/01_03_RC梁_柱の終局せん断耐力_荒川mean式系.md>)（6行、先頭1行目）
- [docs/calc_basis/08_部材終局耐力/01_RC_部材_略算終局耐力.md](<../../docs/calc_basis/08_部材終局耐力/01_RC_部材_略算終局耐力.md>)（15行、先頭1行目）
- [docs/calc_basis/08_部材終局耐力/02_01_RC梁_柱の終局せん断耐力_荒川mean式.md](<../../docs/calc_basis/08_部材終局耐力/02_01_RC梁_柱の終局せん断耐力_荒川mean式.md>)（5行、先頭1行目）
- [docs/calc_basis/08_部材終局耐力/02_02_RC梁_柱の大地震時の付着照査.md](<../../docs/calc_basis/08_部材終局耐力/02_02_RC梁_柱の大地震時の付着照査.md>)（4行、先頭1行目）
- [docs/calc_basis/08_部材終局耐力/02_04_RC柱の軸終局耐力.md](<../../docs/calc_basis/08_部材終局耐力/02_04_RC柱の軸終局耐力.md>)（5行、先頭1行目）
- [docs/calc_basis/08_部材終局耐力/02_RC_終局せん断.md](<../../docs/calc_basis/08_部材終局耐力/02_RC_終局せん断.md>)（21行、先頭1行目）
- [docs/calc_basis/08_部材終局耐力/03_RC_柱梁接合部_終局耐力.md](<../../docs/calc_basis/08_部材終局耐力/03_RC_柱梁接合部_終局耐力.md>)（6行、先頭1行目）
- [docs/calc_basis/08_部材終局耐力/05_SRC_梁_せん断終局.md](<../../docs/calc_basis/08_部材終局耐力/05_SRC_梁_せん断終局.md>)（5行、先頭6行目）
- [docs/calc_basis/08_部材終局耐力/06_04_CFT正方形長柱の終局曲げ耐力_Case2.md](<../../docs/calc_basis/08_部材終局耐力/06_04_CFT正方形長柱の終局曲げ耐力_Case2.md>)（1行、先頭18行目）
- [docs/calc_basis/08_部材終局耐力/07_01_静的_両端ヒンジ略算による終局検定.md](<../../docs/calc_basis/08_部材終局耐力/07_01_静的_両端ヒンジ略算による終局検定.md>)（12行、先頭3行目）
- [docs/calc_basis/08_部材終局耐力/07_02_増分_確定総応答による終局検定.md](<../../docs/calc_basis/08_部材終局耐力/07_02_増分_確定総応答による終局検定.md>)（2行、先頭13行目）
- [docs/calc_basis/08_部材終局耐力/07_終局検定の統括.md](<../../docs/calc_basis/08_部材終局耐力/07_終局検定の統括.md>)（13行、先頭8行目）
- [docs/calc_basis/08_部材終局耐力/08_01_RC壁の耐力算定用断面と壁脚内力.md](<../../docs/calc_basis/08_部材終局耐力/08_01_RC壁の耐力算定用断面と壁脚内力.md>)（6行、先頭1行目）
- [docs/calc_basis/08_部材終局耐力/08_02_RC壁の初期耐力に用いる静的一次参照.md](<../../docs/calc_basis/08_部材終局耐力/08_02_RC壁の初期耐力に用いる静的一次参照.md>)（4行、先頭1行目）
- [docs/calc_basis/08_部材終局耐力/08_03_RC壁の終局せん断耐力_荒川min_mean.md](<../../docs/calc_basis/08_部材終局耐力/08_03_RC壁の終局せん断耐力_荒川min_mean.md>)（10行、先頭1行目）
- [docs/calc_basis/08_部材終局耐力/08_RC壁の壁脚内力と初期耐力.md](<../../docs/calc_basis/08_部材終局耐力/08_RC壁の壁脚内力と初期耐力.md>)（8行、先頭1行目）
- [当時の鋼板耐震壁のせん断耐力](https://github.com/hrntsm/SEPIKA/blob/7fe3b5d59b2861f9c0798875823df0b30735ec8c/docs/calc_basis/08_部材終局耐力/09_鋼板耐震壁のせん断耐力.md#L15)（1行、先頭15行目）
- [docs/calc_basis/08_部材終局耐力/10_01_線材の曲げヒンジ判定用耐力.md](<../../docs/calc_basis/08_部材終局耐力/10_01_線材の曲げヒンジ判定用耐力.md>)（14行、先頭15行目）
- [docs/calc_basis/08_部材終局耐力/10_02_線材のせん断降伏判定用耐力.md](<../../docs/calc_basis/08_部材終局耐力/10_02_線材のせん断降伏判定用耐力.md>)（13行、先頭21行目）
- [docs/calc_basis/08_部材終局耐力/10_線材のヒンジ判定用耐力.md](<../../docs/calc_basis/08_部材終局耐力/10_線材のヒンジ判定用耐力.md>)（1行、先頭6行目）
- [docs/calc_basis/08_部材終局耐力/README.md](<../../docs/calc_basis/08_部材終局耐力/README.md>)（12行、先頭18行目）
- [docs/calc_basis/09_免震制振/02_制振ダンパー.md](<../../docs/calc_basis/09_免震制振/02_制振ダンパー.md>)（1行、先頭58行目）
- [docs/calc_basis/10_数量積算/01_数量積算の算定式.md](<../../docs/calc_basis/10_数量積算/01_数量積算の算定式.md>)（17行、先頭14行目）
- [docs/calc_basis/README.md](<../../docs/calc_basis/README.md>)（6行、先頭55行目）
- [docs/model_edit/02_断面の符号と階.md](<../../docs/model_edit/02_断面の符号と階.md>)（13行、先頭71行目）
- [docs/model_edit/06_画面構成と工程タブ.md](<../../docs/model_edit/06_画面構成と工程タブ.md>)（1行、先頭213行目）
- [docs/model_edit/08_断面用途.md](<../../docs/model_edit/08_断面用途.md>)（8行、先頭25行目）
- [docs/model_edit/09_梁の鉛直偏心.md](<../../docs/model_edit/09_梁の鉛直偏心.md>)（2行、先頭53行目）
- [docs/model_edit/12_床板の入力.md](<../../docs/model_edit/12_床板の入力.md>)（1行、先頭7行目）
- [docs/model_edit/13_壁版の入力.md](<../../docs/model_edit/13_壁版の入力.md>)（2行、先頭24行目）
- [docs/model_io/01_プロジェクト形式_ovika.md](<../../docs/model_io/01_プロジェクト形式_ovika.md>)（26行、先頭96行目）
- [docs/nonlinear/01_一軸材料構成則.md](<../../docs/nonlinear/01_一軸材料構成則.md>)（12行、先頭3行目）
- [docs/nonlinear/02_部材履歴則.md](<../../docs/nonlinear/02_部材履歴則.md>)（5行、先頭11行目）
- [docs/nonlinear/03_ファイバー断面.md](<../../docs/nonlinear/03_ファイバー断面.md>)（11行、先頭60行目）
- [docs/nonlinear/04_非線形梁要素のモデル化.md](<../../docs/nonlinear/04_非線形梁要素のモデル化.md>)（26行、先頭27行目）
- [docs/nonlinear/05_RC_部材_非線形復元力特性.md](<../../docs/nonlinear/05_RC_部材_非線形復元力特性.md>)（28行、先頭1行目）
- [docs/nonlinear/06_壁の非線形モデル.md](<../../docs/nonlinear/06_壁の非線形モデル.md>)（12行、先頭5行目）
- [docs/nonlinear/README.md](<../../docs/nonlinear/README.md>)（2行、先頭10行目）
- [docs/preparation/01_階の分布.md](<../../docs/preparation/01_階の分布.md>)（4行、先頭89行目）
- [docs/preparation/06_仕口パネル.md](<../../docs/preparation/06_仕口パネル.md>)（3行、先頭92行目）
- [docs/preparation/08_幅厚比.md](<../../docs/preparation/08_幅厚比.md>)（1行、先頭16行目）
- [docs/preparation/09_部材剛性.md](<../../docs/preparation/09_部材剛性.md>)（4行、先頭12行目）
- [docs/preparation/README.md](<../../docs/preparation/README.md>)（7行、先頭95行目）
- [docs/result_view/06_検定比の表示.md](<../../docs/result_view/06_検定比の表示.md>)（9行、先頭12行目）
- [docs/result_view/07_モデル化の表示.md](<../../docs/result_view/07_モデル化の表示.md>)（10行、先頭26行目）
- [docs/result_view/10_時刻歴アニメーションの表示.md](<../../docs/result_view/10_時刻歴アニメーションの表示.md>)（12行、先頭78行目）

### ADR（37件）

- [dev_docs/adr/0019-material-wall-section-and-directional-strength.md](<../adr/0019-material-wall-section-and-directional-strength.md>)（1行、先頭7行目）
- [dev_docs/adr/0025-consistent-mass-section-properties.md](<../adr/0025-consistent-mass-section-properties.md>)（2行、先頭7行目）
- [dev_docs/adr/0031-withdraw-high-strength-shear-reinforcement.md](<../adr/0031-withdraw-high-strength-shear-reinforcement.md>)（3行、先頭11行目）
- [dev_docs/adr/0032-concrete-allowable-stress-basis.md](<../adr/0032-concrete-allowable-stress-basis.md>)（1行、先頭26行目）
- [dev_docs/adr/0033-steel-design-weight-vs-physical-mass.md](<../adr/0033-steel-design-weight-vs-physical-mass.md>)（4行、先頭30行目）
- [dev_docs/adr/0034-rc-section-purpose-types-and-real-rebar.md](<../adr/0034-rc-section-purpose-types-and-real-rebar.md>)（3行、先頭1行目）
- [dev_docs/adr/0041-holding-result-input-validity.md](<../adr/0041-holding-result-input-validity.md>)（1行、先頭30行目）
- [dev_docs/adr/0042-attached-slab-validation-and-support-resolution.md](<../adr/0042-attached-slab-validation-and-support-resolution.md>)（3行、先頭35行目）
- [dev_docs/adr/0044-wall-horizontal-ratio-and-component-validation.md](<../adr/0044-wall-horizontal-ratio-and-component-validation.md>)（1行、先頭9行目）
- [dev_docs/adr/0048-rc-beam-reference-total-rotation.md](<../adr/0048-rc-beam-reference-total-rotation.md>)（2行、先頭1行目）
- [dev_docs/adr/0052-stbridge-rebar-source-and-analysis-boundary.md](<../adr/0052-stbridge-rebar-source-and-analysis-boundary.md>)（2行、先頭9行目）
- [dev_docs/adr/0053-src-material-regions-and-discrete-rebar.md](<../adr/0053-src-material-regions-and-discrete-rebar.md>)（1行、先頭7行目）
- [dev_docs/adr/0055-rc-wall-static-face-and-initial-capacity.md](<../adr/0055-rc-wall-static-face-and-initial-capacity.md>)（2行、先頭1行目）
- [dev_docs/adr/0056-rc-wall-shear-method.md](<../adr/0056-rc-wall-shear-method.md>)（4行、先頭1行目）
- [dev_docs/adr/0057-quantity-settings-and-directional-wall-subtotals.md](<../adr/0057-quantity-settings-and-directional-wall-subtotals.md>)（2行、先頭13行目）
- [dev_docs/adr/0058-rc-beam-signed-q0.md](<../adr/0058-rc-beam-signed-q0.md>)（6行、先頭1行目）
- [dev_docs/adr/0059-incomplete-model-reference-integrity.md](<../adr/0059-incomplete-model-reference-integrity.md>)（1行、先頭40行目）
- [dev_docs/adr/0061-ultimate-gravity-demand-modes.md](<../adr/0061-ultimate-gravity-demand-modes.md>)（5行、先頭3行目）
- [dev_docs/adr/0063-wall-side-column-foundations.md](<../adr/0063-wall-side-column-foundations.md>)（2行、先頭19行目）
- [dev_docs/adr/0075-rc-beam-length-weighted-slab-thickness.md](<../adr/0075-rc-beam-length-weighted-slab-thickness.md>)（2行、先頭1行目）
- [dev_docs/adr/0078-equilibrated-cholesky-condition-estimate.md](<../adr/0078-equilibrated-cholesky-condition-estimate.md>)（1行、先頭7行目）
- [dev_docs/adr/0080-retire-reference-skeleton.md](<../adr/0080-retire-reference-skeleton.md>)（3行、先頭7行目）
- [dev_docs/adr/0107-rc-joint-ultimate-unresolved-state.md](<../adr/0107-rc-joint-ultimate-unresolved-state.md>)（2行、先頭1行目）
- [dev_docs/adr/0110-steel-column-explicit-buckling-length.md](<../adr/0110-steel-column-explicit-buckling-length.md>)（1行、先頭11行目）
- [dev_docs/adr/0111-rc-beam-bending-applicability.md](<../adr/0111-rc-beam-bending-applicability.md>)（3行、先頭1行目）
- [dev_docs/adr/0116-steel-girder-slab-composite-applicability.md](<../adr/0116-steel-girder-slab-composite-applicability.md>)（1行、先頭9行目）
- [dev_docs/adr/0119-standard-rebar-designation.md](<../adr/0119-standard-rebar-designation.md>)（2行、先頭1行目）
- [dev_docs/adr/0120-rc-beam-static-hinge-applicability.md](<../adr/0120-rc-beam-static-hinge-applicability.md>)（4行、先頭1行目）
- [dev_docs/adr/0131-src-joint-allowable-shear.md](<../adr/0131-src-joint-allowable-shear.md>)（1行、先頭12行目）
- [dev_docs/adr/0139-rc-beam-ultimate-shear-demand.md](<../adr/0139-rc-beam-ultimate-shear-demand.md>)（4行、先頭1行目）
- [dev_docs/adr/0141-remove-st-bridge.md](<../adr/0141-remove-st-bridge.md>)（1行、先頭19行目）
- [dev_docs/adr/0142-remove-pca-members.md](<../adr/0142-remove-pca-members.md>)（1行、先頭5行目）
- [dev_docs/adr/0143-rc-column-head-shear-factor.md](<../adr/0143-rc-column-head-shear-factor.md>)（2行、先頭1行目）
- [dev_docs/adr/0144-rc-column-qd1-girder-end-conditions.md](<../adr/0144-rc-column-qd1-girder-end-conditions.md>)（3行、先頭1行目）
- [dev_docs/adr/0145-rc-column-long-term-shear-span.md](<../adr/0145-rc-column-long-term-shear-span.md>)（4行、先頭1行目）
- [dev_docs/adr/0146-rc-column-long-term-shear-alpha.md](<../adr/0146-rc-column-long-term-shear-alpha.md>)（5行、先頭1行目）
- [dev_docs/adr/README.md](<../adr/README.md>)（15行、先頭46行目）

### V&V（201件）

- [dev_docs/v_and_v/App表示と検討入力の整理_Issue1059.md](<App表示と検討入力の整理_Issue1059.md>)（8行、先頭14行目）
- [dev_docs/v_and_v/CFT柱の座屈長さ_Issue1400_2026-11.md](<CFT柱の座屈長さ_Issue1400_2026-11.md>)（2行、先頭8行目）
- [dev_docs/v_and_v/CFT柱終局NM_Issue955_原典照合.md](<CFT柱終局NM_Issue955_原典照合.md>)（1行、先頭20行目）
- [dev_docs/v_and_v/CFT適用材料許容耐力_Issue887_原典照合.md](<CFT適用材料許容耐力_Issue887_原典照合.md>)（2行、先頭27行目）
- [dev_docs/v_and_v/CFT鋼管材料GUI_Issue766_2026-10.md](<CFT鋼管材料GUI_Issue766_2026-10.md>)（3行、先頭19行目）
- [dev_docs/v_and_v/DsQu採用条件_Issue872_確認記録.md](<DsQu採用条件_Issue872_確認記録.md>)（2行、先頭3行目）
- [dev_docs/v_and_v/Ds条件不足の保留_Issue871_確認記録.md](<Ds条件不足の保留_Issue871_確認記録.md>)（12行、先頭11行目）
- [dev_docs/v_and_v/NewRC高強度コンクリート外挿_2026-09.md](<NewRC高強度コンクリート外挿_2026-09.md>)（15行、先頭1行目）
- [dev_docs/v_and_v/OVIKA参照ID整合性_Issue678_2026-10.md](<OVIKA参照ID整合性_Issue678_2026-10.md>)（8行、先頭29行目）
- [dev_docs/v_and_v/RCSRC梁柱のせん断補強筋材料区分_Issue1729_照合記録.md](<RCSRC梁柱のせん断補強筋材料区分_Issue1729_照合記録.md>)（10行、先頭1行目）
- [dev_docs/v_and_v/RCSRC梁柱のせん断補強筋降伏強度_Issue1812_確認記録.md](<RCSRC梁柱のせん断補強筋降伏強度_Issue1812_確認記録.md>)（5行、先頭1行目）
- [dev_docs/v_and_v/RC壁Qc_Issue942_採用範囲照合.md](<RC壁Qc_Issue942_採用範囲照合.md>)（13行、先頭1行目）
- [dev_docs/v_and_v/RC壁Qu適用条件_Issue943_調査記録.md](<RC壁Qu適用条件_Issue943_調査記録.md>)（8行、先頭1行目）
- [dev_docs/v_and_v/RC壁せん断耐力方式_Issue572_2026-10.md](<RC壁せん断耐力方式_Issue572_2026-10.md>)（6行、先頭1行目）
- [dev_docs/v_and_v/RC壁ひび割れ耐力_Issue534_2026-10.md](<RC壁ひび割れ耐力_Issue534_2026-10.md>)（6行、先頭1行目）
- [dev_docs/v_and_v/RC壁ゼロせん断スパン_確認記録.md](<RC壁ゼロせん断スパン_確認記録.md>)（7行、先頭1行目）
- [dev_docs/v_and_v/RC壁公開耐力の適用外拒否_Issue969_確認記録.md](<RC壁公開耐力の適用外拒否_Issue969_確認記録.md>)（7行、先頭1行目）
- [dev_docs/v_and_v/RC壁同一faceと初期耐力_Issue617_2026-10.md](<RC壁同一faceと初期耐力_Issue617_2026-10.md>)（10行、先頭1行目）
- [dev_docs/v_and_v/RC壁方向別入力_Issue535_2026-10.md](<RC壁方向別入力_Issue535_2026-10.md>)（4行、先頭1行目）
- [dev_docs/v_and_v/RC壁方向別数量_Issue536_2026-10.md](<RC壁方向別数量_Issue536_2026-10.md>)（5行、先頭1行目）
- [dev_docs/v_and_v/RC壁横筋寄与の縦筋比上限_Issue910_2026-10.md](<RC壁横筋寄与の縦筋比上限_Issue910_2026-10.md>)（7行、先頭1行目）
- [dev_docs/v_and_v/RC壁線形軸曲げ剛性_Issue652_調査記録.md](<RC壁線形軸曲げ剛性_Issue652_調査記録.md>)（25行、先頭1行目）
- [dev_docs/v_and_v/RC専用断面の板参照境界_Issue677_2026-10.md](<RC専用断面の板参照境界_Issue677_2026-10.md>)（17行、先頭1行目）
- [dev_docs/v_and_v/RC引張Qsu_Issue939_適用調査.md](<RC引張Qsu_Issue939_適用調査.md>)（10行、先頭1行目）
- [dev_docs/v_and_v/RC接合部終局の方向と入力不足_Issue1050_確認記録.md](<RC接合部終局の方向と入力不足_Issue1050_確認記録.md>)（7行、先頭1行目）
- [dev_docs/v_and_v/RC接合部許容せん断_Issue948_照合記録.md](<RC接合部許容せん断_Issue948_照合記録.md>)（11行、先頭1行目）
- [dev_docs/v_and_v/RC断面の専用用途_Issue674_2026-10.md](<RC断面の専用用途_Issue674_2026-10.md>)（20行、先頭1行目）
- [dev_docs/v_and_v/RC断面設計n_Issue944_適用照合.md](<RC断面設計n_Issue944_適用照合.md>)（11行、先頭1行目）
- [dev_docs/v_and_v/RC曲げひび割れMc_Issue940_適用調査.md](<RC曲げひび割れMc_Issue940_適用調査.md>)（12行、先頭1行目）
- [dev_docs/v_and_v/RC材料構造細則_Issue881_原典照合.md](<RC材料構造細則_Issue881_原典照合.md>)（17行、先頭1行目）
- [dev_docs/v_and_v/RC材料短期倍率軽量_Issue946_適用調査.md](<RC材料短期倍率軽量_Issue946_適用調査.md>)（18行、先頭1行目）
- [dev_docs/v_and_v/RC柱QD1の接続大梁_Issue1820_照合記録.md](<RC柱QD1の接続大梁_Issue1820_照合記録.md>)（6行、先頭1行目）
- [dev_docs/v_and_v/RC柱QD1の柱側梁端条件_Issue1839_照合記録.md](<RC柱QD1の柱側梁端条件_Issue1839_照合記録.md>)（4行、先頭1行目）
- [dev_docs/v_and_v/RC柱の長期せん断需要_Issue1836_照合記録.md](<RC柱の長期せん断需要_Issue1836_照合記録.md>)（11行、先頭1行目）
- [dev_docs/v_and_v/RC柱の長期せん断需要_Issue1843_検証.md](<RC柱の長期せん断需要_Issue1843_検証.md>)（8行、先頭1行目）
- [dev_docs/v_and_v/RC柱の長期許容せん断力_Issue1841_照合記録.md](<RC柱の長期許容せん断力_Issue1841_照合記録.md>)（12行、先頭1行目）
- [dev_docs/v_and_v/RC柱の長期許容せん断力_Issue1854_検証.md](<RC柱の長期許容せん断力_Issue1854_検証.md>)（6行、先頭1行目）
- [dev_docs/v_and_v/RC柱の静的ヒンジせん断需要_Issue1536_確認記録.md](<RC柱の静的ヒンジせん断需要_Issue1536_確認記録.md>)（4行、先頭1行目）
- [dev_docs/v_and_v/RC柱短期軸応力_Issue1008_調査記録.md](<RC柱短期軸応力_Issue1008_調査記録.md>)（12行、先頭1行目）
- [dev_docs/v_and_v/RC柱純引張_確認記録.md](<RC柱純引張_確認記録.md>)（9行、先頭1行目）
- [dev_docs/v_and_v/RC柱許容曲げゼロ境界_確認記録.md](<RC柱許容曲げゼロ境界_確認記録.md>)（16行、先頭1行目）
- [dev_docs/v_and_v/RC柱頭の梁降伏による設計用せん断力_Issue1800_照合記録.md](<RC柱頭の梁降伏による設計用せん断力_Issue1800_照合記録.md>)（6行、先頭1行目）
- [dev_docs/v_and_v/RC梁Q0の符号付き端反力合成_Issue600_2026-10.md](<RC梁Q0の符号付き端反力合成_Issue600_2026-10.md>)（22行、先頭1行目）
- [dev_docs/v_and_v/RC梁αy_Issue941_採用式照合.md](<RC梁αy_Issue941_採用式照合.md>)（8行、先頭1行目）
- [dev_docs/v_and_v/RC梁αyと追加端ばね_Issue523_2026-10.md](<RC梁αyと追加端ばね_Issue523_2026-10.md>)（27行、先頭1行目）
- [dev_docs/v_and_v/RC梁の曲げ未算定と独立した不適合_Issue1814_確認記録.md](<RC梁の曲げ未算定と独立した不適合_Issue1814_確認記録.md>)（9行、先頭1行目）
- [dev_docs/v_and_v/RC梁の曲げ終局耐力略算_Issue1501_確認記録.md](<RC梁の曲げ終局耐力略算_Issue1501_確認記録.md>)（8行、先頭1行目）
- [dev_docs/v_and_v/RC梁の非有限なあばら筋間隔_Issue1853_検証.md](<RC梁の非有限なあばら筋間隔_Issue1853_検証.md>)（5行、先頭1行目）
- [dev_docs/v_and_v/RC梁の非有限なあばら筋間隔による終局検定_Issue1857_検証.md](<RC梁の非有限なあばら筋間隔による終局検定_Issue1857_検証.md>)（11行、先頭1行目）
- [dev_docs/v_and_v/RC梁両端曲げ降伏時せん断力_Issue1497_確認記録.md](<RC梁両端曲げ降伏時せん断力_Issue1497_確認記録.md>)（11行、先頭1行目）
- [dev_docs/v_and_v/RC梁左右床板厚_Issue618_2026-10.md](<RC梁左右床板厚_Issue618_2026-10.md>)（21行、先頭1行目）
- [dev_docs/v_and_v/RC梁柱の丸鋼主筋の終局付着_Issue1730_照合記録.md](<RC梁柱の丸鋼主筋の終局付着_Issue1730_照合記録.md>)（7行、先頭1行目）
- [dev_docs/v_and_v/RC梁柱の異形鉄筋1段の終局付着_Issue1805_照合記録.md](<RC梁柱の異形鉄筋1段の終局付着_Issue1805_照合記録.md>)（16行、先頭1行目）
- [dev_docs/v_and_v/RC梁非対称多段配筋_圧縮筋位置の確認記録.md](<RC梁非対称多段配筋_圧縮筋位置の確認記録.md>)（5行、先頭1行目）
- [dev_docs/v_and_v/RC片持ち架構の階入力_Issue786_確認記録.md](<RC片持ち架構の階入力_Issue786_確認記録.md>)（6行、先頭1行目）
- [dev_docs/v_and_v/RC耐震ルート_Issue882_原典照合.md](<RC耐震ルート_Issue882_原典照合.md>)（13行、先頭1行目）
- [dev_docs/v_and_v/RC部材力学モデル_Issue883_原典照合.md](<RC部材力学モデル_Issue883_原典照合.md>)（19行、先頭1行目）
- [dev_docs/v_and_v/RC部材引張軸力_Issue697_継続照合.md](<RC部材引張軸力_Issue697_継続照合.md>)（12行、先頭1行目）
- [dev_docs/v_and_v/RC鉄筋径と未指定強度_Issue947_照合記録.md](<RC鉄筋径と未指定強度_Issue947_照合記録.md>)（11行、先頭1行目）
- [dev_docs/v_and_v/RC鉄筋比契約_Issue435_2026-10.md](<RC鉄筋比契約_Issue435_2026-10.md>)（24行、先頭1行目）
- [dev_docs/v_and_v/RC開口壁_Issue821_モデル監査.md](<RC開口壁_Issue821_モデル監査.md>)（26行、先頭1行目）
- [dev_docs/v_and_v/README.md](<README.md>)（160行、先頭5行目）
- [dev_docs/v_and_v/SRC_CFT材料弾性係数_Issue954_原典照合.md](<SRC_CFT材料弾性係数_Issue954_原典照合.md>)（10行、先頭7行目）
- [dev_docs/v_and_v/SRCファイバー解析の内蔵H形鋼と板厚区分_Issue1703_照合記録.md](<SRCファイバー解析の内蔵H形鋼と板厚区分_Issue1703_照合記録.md>)（2行、先頭61行目）
- [dev_docs/v_and_v/SRC内蔵H形鋼の材種_Issue1610_照合記録.md](<SRC内蔵H形鋼の材種_Issue1610_照合記録.md>)（1行、先頭5行目）
- [dev_docs/v_and_v/SRC内蔵鉄骨材料_Issue912_確認記録.md](<SRC内蔵鉄骨材料_Issue912_確認記録.md>)（2行、先頭9行目）
- [dev_docs/v_and_v/SRC増分解析の内蔵H形鋼せん断降伏_Issue1614_照合記録.md](<SRC増分解析の内蔵H形鋼せん断降伏_Issue1614_照合記録.md>)（3行、先頭5行目）
- [dev_docs/v_and_v/SRC接合部許容せん断_Issue949_原典照合.md](<SRC接合部許容せん断_Issue949_原典照合.md>)（12行、先頭11行目）
- [dev_docs/v_and_v/SRC柱RC梁接合部短期許容せん断_Issue1512_原典照合.md](<SRC柱RC梁接合部短期許容せん断_Issue1512_原典照合.md>)（15行、先頭1行目）
- [dev_docs/v_and_v/SRC柱梁接合部帯筋_Issue1513_原典照合.md](<SRC柱梁接合部帯筋_Issue1513_原典照合.md>)（1行、先頭7行目）
- [dev_docs/v_and_v/SRC柱短期地震時の曲げ方式_Issue1416_確認記録.md](<SRC柱短期地震時の曲げ方式_Issue1416_確認記録.md>)（7行、先頭3行目）
- [dev_docs/v_and_v/SRC柱累加耐力_Issue953_原典照合.md](<SRC柱累加耐力_Issue953_原典照合.md>)（20行、先頭11行目）
- [dev_docs/v_and_v/SRC柱部材種別の断面幾何_Issue1613_確認記録.md](<SRC柱部材種別の断面幾何_Issue1613_確認記録.md>)（2行、先頭5行目）
- [dev_docs/v_and_v/SRC柱部材種別材料_Issue1417_確認記録.md](<SRC柱部材種別材料_Issue1417_確認記録.md>)（2行、先頭9行目）
- [dev_docs/v_and_v/SRC柱重力参照不足_確認記録.md](<SRC柱重力参照不足_確認記録.md>)（1行、先頭26行目）
- [dev_docs/v_and_v/SRC梁のQL参照完全性_Issue671_2026-10.md](<SRC梁のQL参照完全性_Issue671_2026-10.md>)（5行、先頭22行目）
- [dev_docs/v_and_v/SRC構造細則一次設計_Issue884_原典照合.md](<SRC構造細則一次設計_Issue884_原典照合.md>)（7行、先頭20行目）
- [dev_docs/v_and_v/SRC耐震ルート_Issue885_原典照合.md](<SRC耐震ルート_Issue885_原典照合.md>)（10行、先頭17行目）
- [dev_docs/v_and_v/SRC解析用弾性係数比_Issue434_2026-10.md](<SRC解析用弾性係数比_Issue434_2026-10.md>)（6行、先頭9行目）
- [dev_docs/v_and_v/SRC靭性終局仕口_Issue886_原典照合.md](<SRC靭性終局仕口_Issue886_原典照合.md>)（12行、先頭12行目）
- [dev_docs/v_and_v/STB_CFT鋼管標準密度_Issue741_2026-10.md](<STB_CFT鋼管標準密度_Issue741_2026-10.md>)（1行、先頭66行目）
- [dev_docs/v_and_v/STB断面系列別ID_Issue571_2026-10.md](<STB断面系列別ID_Issue571_2026-10.md>)（11行、先頭19行目）
- [dev_docs/v_and_v/STB書出の開発中表示_Issue647_2026-10.md](<STB書出の開発中表示_Issue647_2026-10.md>)（1行、先頭21行目）
- [dev_docs/v_and_v/STB標準入力限定_Issue660_2026-10.md](<STB標準入力限定_Issue660_2026-10.md>)（7行、先頭37行目）
- [dev_docs/v_and_v/STB組立BOX暫定拒否_Issue742_2026-10.md](<STB組立BOX暫定拒否_Issue742_2026-10.md>)（1行、先頭80行目）
- [dev_docs/v_and_v/STB配置変換_Issue1367_2026-11.md](<STB配置変換_Issue1367_2026-11.md>)（9行、先頭5行目）
- [dev_docs/v_and_v/S造合成梁剛性_Issue1306_原典照合.md](<S造合成梁剛性_Issue1306_原典照合.md>)（9行、先頭3行目）
- [dev_docs/v_and_v/UI検討_Issue973_接続確認.md](<UI検討_Issue973_接続確認.md>)（2行、先頭43行目）
- [dev_docs/v_and_v/USD685主筋_Issue945_認定条件照合.md](<USD685主筋_Issue945_認定条件照合.md>)（21行、先頭1行目）
- [dev_docs/v_and_v/adversarial_review_2026-07.md](<adversarial_review_2026-07.md>)（3行、先頭117行目）
- [dev_docs/v_and_v/load_calculation_review.md](<load_calculation_review.md>)（12行、先頭72行目）
- [dev_docs/v_and_v/p3_review.md](<p3_review.md>)（16行、先頭19行目）
- [dev_docs/v_and_v/p4_review.md](<p4_review.md>)（7行、先頭173行目）
- [dev_docs/v_and_v/p7_review.md](<p7_review.md>)（3行、先頭66行目）
- [dev_docs/v_and_v/pending_items.md](<pending_items.md>)（9行、先頭116行目）
- [dev_docs/v_and_v/ねじれで柱の層間変位が相殺される場合の偏心率_Issue1595_照合記録.md](<ねじれで柱の層間変位が相殺される場合の偏心率_Issue1595_照合記録.md>)（1行、先頭89行目）
- [dev_docs/v_and_v/コンクリート長期せん断_Issue922_是正記録.md](<コンクリート長期せん断_Issue922_是正記録.md>)（7行、先頭7行目）
- [dev_docs/v_and_v/ファイバー共通処理の試験整理_Issue1059.md](<ファイバー共通処理の試験整理_Issue1059.md>)（1行、先頭38行目）
- [dev_docs/v_and_v/ファイバー形状_長期初期載荷_2026-07.md](<ファイバー形状_長期初期載荷_2026-07.md>)（4行、先頭13行目）
- [dev_docs/v_and_v/ファイバー材料モデル_論文照合_2026-07.md](<ファイバー材料モデル_論文照合_2026-07.md>)（6行、先頭70行目）
- [dev_docs/v_and_v/ファイバー質量と材料試験の整理_Issue1059.md](<ファイバー質量と材料試験の整理_Issue1059.md>)（3行、先頭12行目）
- [dev_docs/v_and_v/フィレット_角R_断面整合_Issue413_2026-10.md](<フィレット_角R_断面整合_Issue413_2026-10.md>)（2行、先頭178行目）
- [dev_docs/v_and_v/フレーム外雑壁の壁版への移行_2026-09.md](<フレーム外雑壁の壁版への移行_2026-09.md>)（3行、先頭49行目）
- [dev_docs/v_and_v/一般部材荷重_Issue716_2026-10.md](<一般部材荷重_Issue716_2026-10.md>)（3行、先頭70行目）
- [dev_docs/v_and_v/主架構支持の解析表現不変性_Issue597_2026-10.md](<主架構支持の解析表現不変性_Issue597_2026-10.md>)（1行、先頭27行目）
- [dev_docs/v_and_v/主架構線材の2節点入力_Issue562_2026-10.md](<主架構線材の2節点入力_Issue562_2026-10.md>)（2行、先頭30行目）
- [dev_docs/v_and_v/二次CFT入口拒否_Issue539_2026-10.md](<二次CFT入口拒否_Issue539_2026-10.md>)（2行、先頭28行目）
- [dev_docs/v_and_v/仕口パネル_定式化と検証_2026-07.md](<仕口パネル_定式化と検証_2026-07.md>)（1行、先頭79行目）
- [dev_docs/v_and_v/仕口パネル参照柱_Issue585_2026-10.md](<仕口パネル参照柱_Issue585_2026-10.md>)（1行、先頭24行目）
- [dev_docs/v_and_v/仕口パネル接合面距離_Issue759_2026-10.md](<仕口パネル接合面距離_Issue759_2026-10.md>)（2行、先頭104行目）
- [dev_docs/v_and_v/仕口パネル柱2軸_Issue757_2026-10.md](<仕口パネル柱2軸_Issue757_2026-10.md>)（3行、先頭124行目）
- [dev_docs/v_and_v/仕口パネル段違い偏心_Issue1365_原典照合.md](<仕口パネル段違い偏心_Issue1365_原典照合.md>)（1行、先頭42行目）
- [dev_docs/v_and_v/仕口パネル状態と生成の試験整理_Issue1059.md](<仕口パネル状態と生成の試験整理_Issue1059.md>)（1行、先頭40行目）
- [dev_docs/v_and_v/仕口パネル耐力式_Issue723_原典調査.md](<仕口パネル耐力式_Issue723_原典調査.md>)（1行、先頭121行目）
- [dev_docs/v_and_v/保存成果の物理曲率と剛域軸履歴_Issue496_2026-10.md](<保存成果の物理曲率と剛域軸履歴_Issue496_2026-10.md>)（1行、先頭11行目）
- [dev_docs/v_and_v/保存成果の計算契約版_Issue587_2026-10.md](<保存成果の計算契約版_Issue587_2026-10.md>)（10行、先頭32行目）
- [dev_docs/v_and_v/保有水平耐力_プッシュオーバー_敵対的レビュー_2026-07.md](<保有水平耐力_プッシュオーバー_敵対的レビュー_2026-07.md>)（22行、先頭7行目）
- [dev_docs/v_and_v/共有入力と反復処理の整理_Issue1059.md](<共有入力と反復処理の整理_Issue1059.md>)（4行、先頭17行目）
- [dev_docs/v_and_v/共通局所座標の参照方向_Issue762_2026-10.md](<共通局所座標の参照方向_Issue762_2026-10.md>)（2行、先頭44行目）
- [dev_docs/v_and_v/共通断面質量_Issue594_2026-10.md](<共通断面質量_Issue594_2026-10.md>)（4行、先頭18行目）
- [dev_docs/v_and_v/内部参照検査境界_Issue681_2026-10.md](<内部参照検査境界_Issue681_2026-10.md>)（1行、先頭94行目）
- [dev_docs/v_and_v/剛域_保有水平耐力_系レベル検証.md](<剛域_保有水平耐力_系レベル検証.md>)（1行、先頭170行目）
- [dev_docs/v_and_v/剛域算定の壁展開順序不整合_2026-08.md](<剛域算定の壁展開順序不整合_2026-08.md>)（9行、先頭45行目）
- [dev_docs/v_and_v/剛性計算_参照実装照合.md](<剛性計算_参照実装照合.md>)（23行、先頭15行目）
- [dev_docs/v_and_v/単独RC壁Ds応力度と側柱寸法_Issue619_2026-10.md](<単独RC壁Ds応力度と側柱寸法_Issue619_2026-10.md>)（13行、先頭1行目）
- [dev_docs/v_and_v/原典照合リスト.md](<原典照合リスト.md>)（44行、先頭6行目）
- [dev_docs/v_and_v/原典照合リスト_2025年版追補_20261027.md](<原典照合リスト_2025年版追補_20261027.md>)（16行、先頭28行目）
- [dev_docs/v_and_v/原階所属_Issue497_2026-10.md](<原階所属_Issue497_2026-10.md>)（3行、先頭52行目）
- [dev_docs/v_and_v/参照骨格の部材角単位_Issue436_2026-10.md](<参照骨格の部材角単位_Issue436_2026-10.md>)（8行、先頭24行目）
- [dev_docs/v_and_v/古い自動荷重と通常保存_Issue816_確認記録.md](<古い自動荷重と通常保存_Issue816_確認記録.md>)（2行、先頭48行目）
- [dev_docs/v_and_v/地震力標準式と適用診断_Issue486_2026-10.md](<地震力標準式と適用診断_Issue486_2026-10.md>)（18行、先頭22行目）
- [dev_docs/v_and_v/地震重量の鮮度と共通準備_Issue439_2026-10.md](<地震重量の鮮度と共通準備_Issue439_2026-10.md>)（8行、先頭36行目）
- [dev_docs/v_and_v/基礎梁型枠_Issue1276_確認記録.md](<基礎梁型枠_Issue1276_確認記録.md>)（2行、先頭21行目）
- [dev_docs/v_and_v/増分解析_ヒンジ形成と剛性低下_検証_2026-07.md](<増分解析_ヒンジ形成と剛性低下_検証_2026-07.md>)（1行、先頭108行目）
- [dev_docs/v_and_v/増分解析_局所処理整理_Issue1059.md](<増分解析_局所処理整理_Issue1059.md>)（1行、先頭21行目）
- [dev_docs/v_and_v/増分解析_長期載荷の接線剛性破綻_2026-07.md](<増分解析_長期載荷の接線剛性破綻_2026-07.md>)（15行、先頭9行目）
- [dev_docs/v_and_v/壁の確定ステップ応答_Issue443_2026-10.md](<壁の確定ステップ応答_Issue443_2026-10.md>)（1行、先頭52行目）
- [dev_docs/v_and_v/壁の質量と配筋観測_Issue1059.md](<壁の質量と配筋観測_Issue1059.md>)（2行、先頭13行目）
- [dev_docs/v_and_v/壁側柱の物理的分担と局所解放_Issue496_2026-10.md](<壁側柱の物理的分担と局所解放_Issue496_2026-10.md>)（7行、先頭5行目）
- [dev_docs/v_and_v/壁有無の固有周期_Issue1161_確認記録.md](<壁有無の固有周期_Issue1161_確認記録.md>)（3行、先頭49行目）
- [dev_docs/v_and_v/壁横筋入力と終局割線骨格_Issue503_2026-10.md](<壁横筋入力と終局割線骨格_Issue503_2026-10.md>)（4行、先頭4行目）
- [dev_docs/v_and_v/壁自重の独立用途配分_Issue444_2026-10.md](<壁自重の独立用途配分_Issue444_2026-10.md>)（2行、先頭19行目）
- [dev_docs/v_and_v/外部ID入力拒否_Issue603_2026-10.md](<外部ID入力拒否_Issue603_2026-10.md>)（4行、先頭30行目）
- [dev_docs/v_and_v/大梁の実柱面断面力_Issue1511_確認記録.md](<大梁の実柱面断面力_Issue1511_確認記録.md>)（2行、先頭7行目）
- [dev_docs/v_and_v/大梁床板実重複控除_Issue616_確認記録.md](<大梁床板実重複控除_Issue616_確認記録.md>)（11行、先頭5行目）
- [dev_docs/v_and_v/小梁偏心別の数量集約_Issue792_2026-10.md](<小梁偏心別の数量集約_Issue792_2026-10.md>)（3行、先頭9行目）
- [dev_docs/v_and_v/小梁設計を分配結果から出す_2026-08.md](<小梁設計を分配結果から出す_2026-08.md>)（1行、先頭71行目）
- [dev_docs/v_and_v/床領域の再設計_荷重分配とSlabFloorRegion分離_2026-08.md](<床領域の再設計_荷重分配とSlabFloorRegion分離_2026-08.md>)（2行、先頭69行目）
- [dev_docs/v_and_v/弾性梁採用G_Issue712_2026-10.md](<弾性梁採用G_Issue712_2026-10.md>)（4行、先頭7行目）
- [dev_docs/v_and_v/応力解析_参照実装照合.md](<応力解析_参照実装照合.md>)（7行、先頭88行目）
- [dev_docs/v_and_v/支持材フェースと共通直交基底_Issue772_2026-10.md](<支持材フェースと共通直交基底_Issue772_2026-10.md>)（2行、先頭40行目）
- [dev_docs/v_and_v/数量積算_参照実装照合.md](<数量積算_参照実装照合.md>)（2行、先頭5行目）
- [dev_docs/v_and_v/整合質量_2026-09.md](<整合質量_2026-09.md>)（3行、先頭20行目）
- [dev_docs/v_and_v/断面検定_参照実装照合.md](<断面検定_参照実装照合.md>)（75行、先頭6行目）
- [dev_docs/v_and_v/明示線材とダンパー総重量_Issue422_2026-10.md](<明示線材とダンパー総重量_Issue422_2026-10.md>)（1行、先頭16行目）
- [dev_docs/v_and_v/時刻歴の実入力と結果所属_Issue611_2026-10.md](<時刻歴の実入力と結果所属_Issue611_2026-10.md>)（1行、先頭56行目）
- [dev_docs/v_and_v/時刻歴時間軸_Issue610_2026-10.md](<時刻歴時間軸_Issue610_2026-10.md>)（1行、先頭101行目）
- [dev_docs/v_and_v/時刻歴簡易検定のRC梁Q0_Issue1646_確認記録.md](<時刻歴簡易検定のRC梁Q0_Issue1646_確認記録.md>)（8行、先頭1行目）
- [dev_docs/v_and_v/未入力材料強度の危険側フォールバック_2026-07.md](<未入力材料強度の危険側フォールバック_2026-07.md>)（6行、先頭32行目）
- [dev_docs/v_and_v/未検証一覧.md](<未検証一覧.md>)（94行、先頭11行目）
- [dev_docs/v_and_v/材料強度_基準強度照合.md](<材料強度_基準強度照合.md>)（3行、先頭41行目）
- [dev_docs/v_and_v/梁の床板嵩上げ_Issue731_2026-10.md](<梁の床板嵩上げ_Issue731_2026-10.md>)（7行、先頭25行目）
- [dev_docs/v_and_v/梁型枠接触和集合_Issue451_2026-10.md](<梁型枠接触和集合_Issue451_2026-10.md>)（1行、先頭3行目）
- [dev_docs/v_and_v/梁鉛直偏心_Issue726_確認記録.md](<梁鉛直偏心_Issue726_確認記録.md>)（2行、先頭9行目）
- [dev_docs/v_and_v/梁鉛直偏心接続_Issue734_確認記録.md](<梁鉛直偏心接続_Issue734_確認記録.md>)（4行、先頭151行目）
- [dev_docs/v_and_v/標準強度_Issue520_2026-10.md](<標準強度_Issue520_2026-10.md>)（2行、先頭69行目）
- [dev_docs/v_and_v/標準配筋_Issue507_2026-10.md](<標準配筋_Issue507_2026-10.md>)（10行、先頭5行目）
- [dev_docs/v_and_v/正定値求解の数値条件_Issue818_確認記録.md](<正定値求解の数値条件_Issue818_確認記録.md>)（7行、先頭7行目）
- [dev_docs/v_and_v/異形鉄筋の公称値_Issue1580_照合記録.md](<異形鉄筋の公称値_Issue1580_照合記録.md>)（21行、先頭9行目）
- [dev_docs/v_and_v/異形鉄筋長期付着_Issue923_2026-10.md](<異形鉄筋長期付着_Issue923_2026-10.md>)（6行、先頭5行目）
- [dev_docs/v_and_v/目的別確定層力_Issue442_2026-10.md](<目的別確定層力_Issue442_2026-10.md>)（4行、先頭64行目）
- [dev_docs/v_and_v/終局検定_参照実装照合.md](<終局検定_参照実装照合.md>)（35行、先頭11行目）
- [dev_docs/v_and_v/終局重力組合せと応力割増し_Issue958_照合記録.md](<終局重力組合せと応力割増し_Issue958_照合記録.md>)（10行、先頭9行目）
- [dev_docs/v_and_v/終局需要の重力契約_Issue565_2026-10.md](<終局需要の重力契約_Issue565_2026-10.md>)（19行、先頭3行目）
- [dev_docs/v_and_v/線材と集中ばねの試験整理_Issue1059.md](<線材と集中ばねの試験整理_Issue1059.md>)（3行、先頭9行目）
- [dev_docs/v_and_v/線材荷重と端部変換の試験整理_Issue1059.md](<線材荷重と端部変換の試験整理_Issue1059.md>)（1行、先頭36行目）
- [dev_docs/v_and_v/耐震壁_敵対的レビュー_2026-07.md](<耐震壁_敵対的レビュー_2026-07.md>)（14行、先頭7行目）
- [dev_docs/v_and_v/耐震壁_未検定_Issue495_2026-10.md](<耐震壁_未検定_Issue495_2026-10.md>)（8行、先頭9行目）
- [dev_docs/v_and_v/自立壁Rs接続_Issue740_調査記録.md](<自立壁Rs接続_Issue740_調査記録.md>)（2行、先頭54行目）
- [dev_docs/v_and_v/自立壁n倍法_Issue620_確認記録.md](<自立壁n倍法_Issue620_確認記録.md>)（3行、先頭10行目）
- [dev_docs/v_and_v/荷重状態と検定対象_Issue487_2026-10.md](<荷重状態と検定対象_Issue487_2026-10.md>)（5行、先頭21行目）
- [dev_docs/v_and_v/荷重責務と剛域切替_Issue853.md](<荷重責務と剛域切替_Issue853.md>)（2行、先頭34行目）
- [dev_docs/v_and_v/要素にならない壁版の可視化と耐震スリット_2026-09.md](<要素にならない壁版の可視化と耐震スリット_2026-09.md>)（1行、先頭57行目）
- [dev_docs/v_and_v/要素共通処理の試験整理_Issue1059.md](<要素共通処理の試験整理_Issue1059.md>)（1行、先頭34行目）
- [dev_docs/v_and_v/設計検定の共通処理と試験整理_Issue1059.md](<設計検定の共通処理と試験整理_Issue1059.md>)（7行、先頭10行目）
- [dev_docs/v_and_v/部材荷重方向の計算入口拒否_Issue687_2026-10.md](<部材荷重方向の計算入口拒否_Issue687_2026-10.md>)（3行、先頭9行目）
- [dev_docs/v_and_v/鉄骨ルート2筋かい応力_Issue952_照合記録.md](<鉄骨ルート2筋かい応力_Issue952_照合記録.md>)（1行、先頭22行目）
- [dev_docs/v_and_v/鉄骨材料と許容応力度_Issue879_原典照合.md](<鉄骨材料と許容応力度_Issue879_原典照合.md>)（1行、先頭163行目）
- [dev_docs/v_and_v/鉄骨柱脚接合構造細則_Issue878_原典照合.md](<鉄骨柱脚接合構造細則_Issue878_原典照合.md>)（1行、先頭31行目）
- [dev_docs/v_and_v/鉄骨柱自動K_適用停止の検証.md](<鉄骨柱自動K_適用停止の検証.md>)（1行、先頭64行目）
- [dev_docs/v_and_v/鋼RC大梁略算たわみ_Issue1195_照合記録.md](<鋼RC大梁略算たわみ_Issue1195_照合記録.md>)（16行、先頭1行目）
- [dev_docs/v_and_v/鋼ブレースXML文字子_Issue550_2026-10.md](<鋼ブレースXML文字子_Issue550_2026-10.md>)（3行、先頭11行目）
- [dev_docs/v_and_v/鋼断面形状とコンクリート主材料_Issue1290_照合記録.md](<鋼断面形状とコンクリート主材料_Issue1290_照合記録.md>)（11行、先頭10行目）
- [dev_docs/v_and_v/鋼材F選択と入力停止_Issue601_2026-10.md](<鋼材F選択と入力停止_Issue601_2026-10.md>)（2行、先頭49行目）
- [dev_docs/v_and_v/階FL_SL_Issue729_確認記録.md](<階FL_SL_Issue729_確認記録.md>)（3行、先頭54行目）
- [dev_docs/v_and_v/集中ばね梁の部材荷重_Issue802_2026-10.md](<集中ばね梁の部材荷重_Issue802_2026-10.md>)（4行、先頭34行目）
- [dev_docs/v_and_v/静的検定の生成入力_Issue602_2026-10.md](<静的検定の生成入力_Issue602_2026-10.md>)（1行、先頭23行目）
- [dev_docs/v_and_v/非節点二次部材数量_Issue557_2026-10.md](<非節点二次部材数量_Issue557_2026-10.md>)（4行、先頭28行目）
- [dev_docs/v_and_v/非線形モデル_参照実装照合.md](<非線形モデル_参照実装照合.md>)（41行、先頭5行目）
- [dev_docs/v_and_v/非線形動的解析_参照実装照合.md](<非線形動的解析_参照実装照合.md>)（7行、先頭55行目）
- [dev_docs/v_and_v/非線形材料と塑性率_Issue956_照合記録.md](<非線形材料と塑性率_Issue956_照合記録.md>)（13行、先頭1行目）

### その他（2件）

- [CONTEXT.md](<../../CONTEXT.md>)（18行、先頭81行目）
- [CONTRIBUTING.md](<../../CONTRIBUTING.md>)（11行、先頭82行目）
