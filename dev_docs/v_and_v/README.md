# V&V レポート（検証・妥当性確認）

## 荷重責務と剛域切替（#853）

[検証記録と要素→試験索引](荷重責務と剛域切替_Issue853.md)は、壁展開のcore移設と、建物共通剛域ON/OFFの剛性・実荷重・CMQ表示への接続を扱う。状態は🔶。手動GUIと実物Validation、増分・時刻歴の全履歴の独立理論解は未実施。


参考用スケルトン計算は [#849](https://github.com/hrntsm/SEPIKA/issues/849) で廃止した。
旧クレート・API・試験・コマンドに言及する記録は実施時点の履歴であり、現在の実行対象ではない。
#436の単位是正と検証の実施記録は維持する。廃止判断と現行計算の保証対応は
[ADR 0080](../adr/0080-retire-reference-skeleton.md) を参照。

## STB異厚組立BOX（#683）

[読込確認記録と要素→テスト索引](STB異厚組立BOX_Issue683_確認記録.md)は、通常鋼・CFTの両公開入口、両板厚・断面参照・属性報告・質量・OVIKA保持とApp受理/失敗時原子性を扱う。状態は🔶。物理Validation・手動GUI・全XSD適合は未確認。

## 異厚組立BOX（#831）

[検証記録と要素→テスト索引](異厚組立BOX_Issue831_確認記録.md)は、鋭角の鋼材・CFT充填部の幾何、材料別質量、実板厚F・接合部方向別板厚、編集・保存・再採番を扱う。状態は🔶。J・せん断式とCFT等価剛性への適用は未確認のため理由付き未算定とし、物理Validation・手動GUIは未確認。


## CFT鋼管材料E（#767）

[検証記録](CFT鋼管材料E_Issue767_2026-10.md)は公開箱・円／長短期の材料E、各Λ境界、数値表現不能と旧直接設計成果の契約失効を扱う。要素→試験索引は同記録を参照する。状態は🔶で、原典本文・非標準材料の物理Validation・手動GUIは未確認。

## RC片持ち架構の明示階入力（#786）

[確認記録](RC片持ち架構の階入力_Issue786_確認記録.md)は、元の回転拘束付き自動剛床の成功、追加拘束なしのねじり保持モデルの独立変位・支持釣合い、既定解放で不安定となる架構のApp拒否をVerificationする。状態は🔶。要素→テスト索引は同記録の自動回帰表を参照する。[無階直接solverの特異性見逃し（#818）](https://github.com/hrntsm/SEPIKA/issues/818)、一般の解放ルール、物理Validation・手動GUIは未確認として保持する。

本ディレクトリは、SEPIKA の各要素・各設計式に対する V&V（Verification & Validation）レポートを格納する。

## 更新手順

- レポートを追加・更新したら、本書のレポート目録と要素→テスト索引の状態（✅/🔶/❌）を更新する。
- 未検証・一部検証の項目は [未検証一覧.md](未検証一覧.md) に反映し、完了した項目は一覧から除く。
- PR 作成前に、PR 本文の残課題と一覧の内容を一致させる。

## 記録時点と最終ゲート

各レポートの試験値・件数・独立期待値・未実施範囲は、記載した実行時点の履歴証拠として保持する。
「後続」「未完了」もその記録時点の状態であり、現在の開発課題の状態はGitHub Issueを参照する。
独立レビュー・親監査・最終head CI・mergeの最新証拠は公開PRで確認し、索引から参照する。
PR作成前のコミットへ、そのPRの将来のCI成功・mergeを予記しない。
索引の🔶は実装・GUI・Validation等の残る範囲を表し、関連Issueの開発作業が未完了であることを一律には意味しない。

今回のループの対象について、2026-10-15に各Issueの完了記録、PRの独立レビュー・親監査記録、
最終headとの一致、CI全5ジョブ・全ステップとdocs build全ステップの成功、merge済みを照合した。
docs deployはPRでは適用外のskipであり、公開サイトへのdeploy成功とは扱わない。
各レポートのローカル失敗・CLI除外・未実施範囲は、このCI成功で取り消さない。

| Issue | 最終ゲート・完了記録のPR | 最終head | CI | docs build |
| --- | --- | --- | --- | --- |
| [#629](https://github.com/hrntsm/SEPIKA/issues/629) | [PR #646](https://github.com/hrntsm/SEPIKA/pull/646) | `b36d2b44` | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38246625228) | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38246625277) |
| [#550](https://github.com/hrntsm/SEPIKA/issues/550) | [PR #648](https://github.com/hrntsm/SEPIKA/pull/648) | `c0660d3b` | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38253749196) | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38253749042) |
| [#433](https://github.com/hrntsm/SEPIKA/issues/433) | [PR #649](https://github.com/hrntsm/SEPIKA/pull/649) | `ec878cb5` | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38313640429) | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38313640446) |
| [#647](https://github.com/hrntsm/SEPIKA/issues/647) | [PR #650](https://github.com/hrntsm/SEPIKA/pull/650) | `9339ab5c` | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38318340297) | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38318340343) |
| [#589](https://github.com/hrntsm/SEPIKA/issues/589) | [PR #651](https://github.com/hrntsm/SEPIKA/pull/651) | `2a1fec94` | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38415406271) | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38415404959) |
| [#535](https://github.com/hrntsm/SEPIKA/issues/535) | [PR #653](https://github.com/hrntsm/SEPIKA/pull/653) | `c66ddf46` | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38428431951) | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38428431710) |
| [#617](https://github.com/hrntsm/SEPIKA/issues/617) | [PR #655](https://github.com/hrntsm/SEPIKA/pull/655) | `f00d9d4a` | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38563484411) | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38563484442) |
| [#572](https://github.com/hrntsm/SEPIKA/issues/572) | [PR #656](https://github.com/hrntsm/SEPIKA/pull/656) | `bd007199` | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38580224172) | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38580224235) |
| [#534](https://github.com/hrntsm/SEPIKA/issues/534) | [PR #658](https://github.com/hrntsm/SEPIKA/pull/658) | `c3e1c3d8` | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38602388828) | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38602388840) |
| [#536](https://github.com/hrntsm/SEPIKA/issues/536) | [PR #659](https://github.com/hrntsm/SEPIKA/pull/659) | `d8d1161e` | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38650324159) | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38650324040) |
| [#475](https://github.com/hrntsm/SEPIKA/issues/475) | [PR #661](https://github.com/hrntsm/SEPIKA/pull/661) | `dc2b0331` | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38656317464) | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38656317463) |
| [#500](https://github.com/hrntsm/SEPIKA/issues/500) | [PR #662](https://github.com/hrntsm/SEPIKA/pull/662) | `e89c1e08` | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38660349205) | [成功](https://github.com/hrntsm/SEPIKA/actions/runs/38660349252) |

既存Rustdoc警告の後続対応 [#654](https://github.com/hrntsm/SEPIKA/issues/654) も [PR #657](https://github.com/hrntsm/SEPIKA/pull/657) で完了済み（[CI成功](https://github.com/hrntsm/SEPIKA/actions/runs/38588087499)、[docs build成功](https://github.com/hrntsm/SEPIKA/actions/runs/38588087778)）。#617・#572当時の通常生成時の警告やstrict試行失敗の記録は保持する。

## クイックリンク

| 一覧 | 内容 |
|------|------|
| [**未検証一覧**](未検証一覧.md) | ❌/🔶 の集約チェックリスト（パッと見る用） |
| [§レポート目録](#レポート目録) | 各 `.md` レポートへの索引 |
| [§索引（要素→テスト）](#索引要素設計式--テスト) | コード上のテスト対応表（#1–#38） |
| [原典照合リスト.md](原典照合リスト.md) | 法令・規準の埋め込み値の専門家サインオフ用チェックリスト（実装者は使わない） |
| [pending_items.md](pending_items.md) | P9 仕様乖離の歴史的記録（訂正履歴含む） |

## V&V の定義

| 用語 | 意味 |
|------|------|
| Verification（検証） | 「式を正しく解いているか」。理論解・手計算・規準例題との一致で確認 |
| Validation（妥当性確認） | 「正しい現象を表しているか」。実験・実測との一致で確認 |

## 検証根拠の区別

各レポートでは、コード・独立した手計算やテスト・公開元を確認した資料を区別する。
匿名の方式比較は、条件と確認範囲が再現できる場合に残す。
独立した裏付けを確認できない判定は、規準適合や計算精度の確認済み判定として扱わない。
採用値・式の一次資料照合の不足は[Issue #697](https://github.com/hrntsm/SEPIKA/issues/697)で追跡する。
文書中の資料の扱いは[共通執筆規約](../README.md)に従う。

## 正定値求解の数値条件（#818）

[確認記録と要素→テスト索引](正定値求解の数値条件_Issue818_確認記録.md)は、RCの独立した不安定性・保持時の既知ねじり、有限弱剛性・単位規模・自由度置換・共有solverの回帰と条件数診断の性能を扱う。有限Verificationは実施、手動GUI・物理Validation・任意行列への数学的保証は対象外で状態は🔶。

## 層間変形角（#490）

[実使用経路の検証](層間変形角_Issue490_2026-10.md)は、上下階SL変位と共通FL間高さによる柱最大角、支配対象、対象なし・未計算・内部不整合、GUI・CSVの対応をVerificationする。手動GUI・実験Validation・法令適用資格は未検証であり、状態は🔶。対応テストは同記録の独立期待値表を要素→テスト索引とする。

## 層指標の生成入力照合（#810）

[検証記録](層指標の生成入力照合_Issue810_2026-10.md)は、GUI/CSVのRs/Re/Fesと使用する方向別静的・長期成果の対応を扱う。手動GUI・物理Validationは未実施で状態は🔶。

## 壁自重の用途配分

[Issue #444 の独立幾何・実入口検証](壁自重の独立用途配分_Issue444_2026-10.md)。DL支持、階帯、共通physical/matrix総量を別々に照合する。局所質量分布の精度と未閲覧原本は未検証として保持する。

## レポート構造

各エントリは以下の項目を持つ:

- **対象**: 検証対象（例: ティモシェンコ梁 / パネルゾーン / Ai分布 / プッシュオーバー機構）
- **参照解の出典**: 理論式 / 実験 / 匿名の結果比較 / 規準例題（公開元と確認範囲を記録）
- **入力モデル**: 再現可能な定義（テストに対応づけ）
- **許容差**: 厳密=1e-9 / 収束=±% / 規準例題照合
- **結果**: 合否・差分・グラフ

## 検証の性格

| 区分 | 該当項目 | 許容差 |
|------|----------|--------|
| 厳密一致 | IIE 梁・CMQ・σ=M/Z・剛性率 | 1e-9 |
| 収束 | MITC4 板・固有値・時刻歴 | ±5% |
| 規準例題照合 | Ai・Ds・許容応力度 | 告示値一致 |

## テスト階層

| レベル | 内容 | ツール |
|--------|------|--------|
| 単体 | 要素剛性・履歴則・断面算定式 | `cargo test`, `approx` |
| 性質 | 剛性対称性・エネルギー保存・パッチテスト | `proptest` |
| 回帰 | 履歴ループ・スケルトン形状 | `insta`（スナップショット） |
| 数値照合 | 理論解（梁・板・SDOF/MDOF） | 専用ベンチ集 |
| ベンチマーク照合 | 既往実験・商用ソフト | 検証レポート |
| 性能 | 速度回帰 | `criterion` + CI 閾値 |
| 決定性 | 同一入力ビット一致 | 専用テスト |

## 塑性確定点のバイリニア接線（#808）

[検証記録](塑性確定点のバイリニア接線_Issue808_2026-10.md)は材料の正負方向微分・応力と塑性履歴の保持、集中梁と瞬間剛性比例減衰への定量伝播を扱う。状態は🔶。同一点の返却接線を固定せず、減衰用接線の採用判断は[Issue #808](https://github.com/hrntsm/SEPIKA/issues/808)に残る。公開時刻歴の有限例で不収束は再現せず、一般応答・累積減衰・Validationは未確認。

## 仕口パネル耐力式の原典調査（#723）

[原典調査と要素→証拠索引](仕口パネル耐力式_Issue723_原典調査.md)に、2025解説書第2版第1刷の原画像・N/A/Fと現行降伏式の対応、全塑性式との区別、CFT調査#888との境界を記録する。状態は🔶。指定原典の関連節は閲覧済みだが、別原典による式・適用条件の採否は未確定で#723は未完了。

## レポート目録

各レポートの照合結果・修正履歴・残課題の詳細。**未完了の要約**は
[未検証一覧.md](未検証一覧.md) を参照。

| レポート | 対象 | 状態 |
|---|---|---|
| [仕口パネル耐力式_Issue723_原典調査.md](仕口パネル耐力式_Issue723_原典調査.md) | 2025解説書の直接記載と別原典の境界、N/A/F・降伏/全塑性の採否 | 🔶（関連原画像閲覧済み。式全体の原典適合・採否未確定） |
| [RC開口壁_Issue821_モデル監査.md](RC開口壁_Issue821_モデル監査.md) | 開口48条件の実入口、壁柱分担・不成立分岐、独立断面幾何と公開原図 | 🔶（現行Verification。適用範囲・安全性・モデル採否未確定） |
| [RC壁線形軸曲げ剛性_Issue652_調査記録.md](RC壁線形軸曲げ剛性_Issue652_調査記録.md) | 現行ps換算と総断面候補、2025年版解説書の指定実画像・材料役割・実入口V&V条件 | 🔶（三本柱の分担と適用上の注意を限定照合。ps換算／総断面採用条件は本書では照合不可。候補未採用・実入口V&V未実施） |
| [RC片持ち架構の階入力_Issue786_確認記録.md](RC片持ち架構の階入力_Issue786_確認記録.md) | 自動剛床とRC片持ち架構の独立変位・支持釣合い、ねじり解放で不安定となる架構の拒否 | 🔶（数値Verification。物理Validation・手動GUI・一般特異性判定は記録参照） |
| [古い自動荷重と通常保存_Issue816_確認記録.md](古い自動荷重と通常保存_Issue816_確認記録.md) | 梁短縮後の既存自動DLと通常保存・再読込、手入力検証・失敗時状態保持・直接検定保存 | 🔶（公開編集/保存入口の自動Verification。手動GUI・物理Validation・最終ゲート未確認） |
| [自立壁n倍法_Issue620_確認記録.md](自立壁n倍法_Issue620_確認記録.md) | 自立壁n倍法の対象・入力・同一断面柱集計 | 🔶（独立Verification。2025限定照合は#740、採用整合・動的専用比較・実測Validation未） |
| [自立壁Rs接続_Issue740_調査記録.md](自立壁Rs接続_Issue740_調査記録.md) | 2025原典のD値式・Rs/Re考慮必要、現行量の定義差・連成2例・Rs正規化 | 🔶（2025限定照合済、一般厳密式は不成立。現行量への近似適用根拠・精度は未確定、#740はblocked・未完了） |
| [梁の床板嵩上げ_Issue731_2026-10.md](梁の床板嵩上げ_Issue731_2026-10.md) | 中心線上端・投影幅、床板材料別の嵩上げ重量/質量、区間DL・小梁設計・動的層質量 | 🔶（独立Verification、手動GUI・Validation未） |
| [RC梁左右床板厚_Issue618_2026-10.md](RC梁左右床板厚_Issue618_2026-10.md) | RC梁の左右床板実厚・長さ平均/丸め上下界・矩形集計・実配置と未解決/複数梁Attached診断 | 🔶（単体/公開入口・床板群の独立Verificationと危険側例。Validation・手動GUI・最終ゲートは記録参照） |
| [層指標の生成入力照合_Issue810_2026-10.md](層指標の生成入力照合_Issue810_2026-10.md) | GUI/CSVのRs/Re/Fes依存成果と現在入力・計算契約 | 🔶（自動Verificationの記録は同レポート、手動GUI・Validation未） |
| [保存ブランチ496の変更採否_2026-10.md](保存ブランチ496の変更採否_2026-10.md) | 全102コミットの採否、免震/保存の復元、全固定時刻歴と確定前記録 | 🔶（有限Verification。実験Validationは未実施） |
| [保存成果の物理曲率と剛域軸履歴_Issue496_2026-10.md](保存成果の物理曲率と剛域軸履歴_Issue496_2026-10.md) | 物理曲率・非対称断面・剛域付き軸履歴、元端部解放の内外残差整合 | 🔶（有限Verification。全履歴の収束性・実験Validationは未検証） |
| [塑性確定点のバイリニア接線_Issue808_2026-10.md](塑性確定点のバイリニア接線_Issue808_2026-10.md) | 同一点の非微分性・正負方向別接線・履歴保持・公開減衰行列への伝播 | 🔶（減衰用接線の採用判断・一般応答は未確認） |
| [集中ばね梁の部材荷重_Issue802_2026-10.md](集中ばね梁の部材荷重_Issue802_2026-10.md) | 集中ばね梁の元部材荷重・材料釣合い・記録・復元・保存失効・公開App保有耐力 | 🔶（独立Verification、Validation・最終ゲート未確認） |
| [一般部材荷重_Issue716_2026-10.md](一般部材荷重_Issue716_2026-10.md) | 全固定材・通常Fiber/MS・非線形時刻歴の長期部材荷重 | 🔶（公開入口と独立梁値のVerification。実測Validation・手動GUI・最終ゲートは未確認） |
| [増分性能量と物理支持反力_Issue717_2026-10.md](増分性能量と物理支持反力_Issue717_2026-10.md) | 全支持抵抗と骨組切断面・元荷重・支持作用の区別 | 🔶（有限例のVerification。一般MPC・全塑性履歴・物理Validation・最終ゲートは未確認） |
| [大梁床板実重複控除_Issue616_確認記録.md](大梁床板実重複控除_Issue616_確認記録.md) | 矩形RC/SRC大梁と床板の実重複・左右幅配分・DL/地震重量・実準備入口・編集保存失効 | 🔶（独立算術と自動入口のVerification。実測Validation・手動GUI・CIは未確認） |
| [梁鉛直偏心接続_Issue734_確認記録.md](梁鉛直偏心接続_Issue734_確認記録.md) | 偏心の剛性・内力・荷重・反力・既存質量・地動入力 | 🔶（数値Verification、Validation・最終CIは記録参照） |
| [梁鉛直偏心_Issue726_確認記録.md](梁鉛直偏心_Issue726_確認記録.md) | 節点材軸基準の一律偏心・有限実配置・GUI/保存/Undo | 🔶（直接Verification、最終横断検証・レビュー・CIは記録参照） |
| [階FL_SL_Issue729_確認記録.md](階FL_SL_Issue729_確認記録.md) | FL／SL・用途別階高と明示入力、GUI編集・保存・Undo・元STB属性 | 🔶（階情報の直接Verification。#490の算定と個別偏心は対象外、最終レビュー・CIは未確認） |
| [梁要素の局所軸と保存性能_Issue760_2026-10.md](梁要素の局所軸と保存性能_Issue760_2026-10.md) | 梁文書の回転軸・変位/せん断方向・保存性能の添字、独立座標図と既存片持ち試験 | ✅（文書是正範囲のVerification。一般形状の精度・物理Validationを保証しない。最終ゲートは後続） |
| [支持材フェースと共通直交基底_Issue772_2026-10.md](支持材フェースと共通直交基底_Issue772_2026-10.md) | 補完なし共通直交姿勢と支持面・区間・荷重・需要、解析40・検定26 | 🔶（公開赤、共有targetの別試験集合観測、専用targetのcore585・荷重/App焦点・旧OVIKA成果失効を区別。GUI全構成・Validation・最終headゲートは記録を参照） |
| [ctutils推移依存監査_Issue775_2026-10.md](ctutils推移依存監査_Issue775_2026-10.md) | ZIP 経由の ctutils 修正済み解決、既存監査ポリシー、OVIKA 回帰 | 🔶（最小ロック差分・監査成功・IO default/parquet 240/251 件成功。最終 PR ゲートは記録を参照） |
| [支持材フェースの有限参照方向_Issue769_2026-10.md](支持材フェースの有限参照方向_Issue769_2026-10.md) | 有限極大参照方向の支持面・小梁自重作用区間・荷重／検定需要、解析39・検定25 | 🔶（現main公開支持面の赤再現と修正後の支持面・荷重・直接検定・保存失効、先行core/load全件・App GUI880件と後続平行丸め境界core584・App契約29件のVerification。途中失敗、最終全件・Validation・最終ゲートは記録を参照） |
| [共通局所座標の参照方向_Issue762_2026-10.md](共通局所座標の参照方向_Issue762_2026-10.md) | 一般線材の断面参照方向、公開計算入口、解析38・検定24 | 🔶（変更前再現、対象RC Q0 8件・限定helper後App GUI lib880件、旧直接検定23の実OVIKA失効、未参照梁の保持と実使用部材の拒否をVerification。先行878件・880件と途中失敗、#769の未対応、Validation・最終ゲートは記録を参照） |
| [CFT鋼管材料GUI_Issue766_2026-10.md](CFT鋼管材料GUI_Issue766_2026-10.md) | CFT角形・円形の鋼管材料表示・編集・Undo/保存・重量検定再計算 | 🔶（egui実表・App直接Verification。手動GUI・Validation・独立レビュー・最終ゲート未実施） |
| [鋼材F選択と入力停止_Issue601_2026-10.md](鋼材F選択と入力停止_Issue601_2026-10.md) | 通常鋼材・鋼小梁・S/CFTの材料/実厚/Fと不備停止・検定23 | 🔶（変更前再現と材料/パネルdesign674・core対象49・Clippy成功。App全479成功/既存1ignore、直接4成功。MCP feature144成功。GUI971成功/既存1ignoreはいずれも形状不足追補前。追補後design675/App4/MCP1とClippy成功。文書検査/build/fmt成功。Validation・最終ゲートは未確認） |
| [微小相似Fiber初期断面_Issue783_2026-10.md](微小相似Fiber初期断面_Issue783_2026-10.md) | 微小断面分割・無次元化・局所後退誤差、公開Auto時刻歴と解析42失効 | 🔶（独立A/両軸I・連成弾性解・尺度変更・失敗復元のVerification。Validationと最終ゲートは記録を参照） |
| [仕口パネル接合面距離_Issue759_2026-10.md](仕口パネル接合面距離_Issue759_2026-10.md) | 一意な実接合面の方向別距離・剛体接続・公開入口と保存失効 | 🔶（all-targets1716/IO240と追補App全tests1024成功。荷重対照と壁fixtureを記録、既存strictdoc警告#800・Validation・最終ゲートは同記録） |
| [仕口パネル実軸力比_Issue761_2026-10.md](仕口パネル実軸力比_Issue761_2026-10.md) | 実材長・実基準軸耐力による軸力比と小寸法・相似尺度 | 🔶（公開要素Verification、solver・保存成果と最終ゲートは同記録。Validation未） |
| [傾斜柱仕口パネル_Issue764_調査記録.md](傾斜柱仕口パネル_Issue764_調査記録.md) | 傾斜柱のMSなし材端力組立の公開再現・独立自由体・再開条件 | 🔶（実装前調査。境界の仕事共役・耐力高さ・状態付き区間荷重の判断待ち。製品式は未変更） |
| [仕口パネル検定2軸集約_Issue758_2026-10.md](仕口パネル検定2軸集約_Issue758_2026-10.md) | S/CFT検定の符号付き主軸集約・参加構面の高さ・検定成果失効 | 🔶（極小nz修正後の直接11/design665・App保存3、先行App全475・関連ClippyのVerification。非鉛直MSなしの未対応域、原典適合・Validation・最終ゲートを区別） |
| [仕口パネル柱2軸_Issue757_2026-10.md](仕口パネル柱2軸_Issue757_2026-10.md) | 柱2主軸と仕事共役な座標・部材接続・方向別成果 | 🔶（CIのjob診断期待を追随しjob159・workspace3851＋doc2成功/4 ignored、関連Clippy・文書検査成功。不正ref修正後の直接証拠と先行GUIを区別。Validation・最終headゲート未確認） |
| [仕口パネル高軸力降伏域_Issue753_2026-10.md](仕口パネル高軸力降伏域_Issue753_2026-10.md) | 高軸力の公開降伏値・実材料閾値と適用域、旧解析成果の利用制限 | 🔶（旧下限・直接5・element516・solver14・App保存契約のVerification。途中試行伝播/先行失敗は同記録、Validation/最終ゲート未） |
| [仕口パネルrevert復元_Issue751_2026-10.md](仕口パネルrevert復元_Issue751_2026-10.md) | 軸力依存降伏値を含む確定状態への直接revertと再試行 | 🔶（直接Verification3・element511成功。関連検証は同レポート、連成接線・Validationは別範囲、高nは#7h） |
| [仕口パネル軸力と履歴_Issue724_2026-10.md](仕口パネル軸力と履歴_Issue724_2026-10.md) | 試行変位による軸力推定・耐力更新・連成接線・履歴と失敗復元 | 🔶（独立Verification4・element508/solver380・Clippy成功。赤再現2件は#751/#752、高n下限は算術のみで#753へ分離。原典§6・Validation・最終ゲート未確認） |
| [STB異厚組立BOX_Issue683_確認記録.md](STB異厚組立BOX_Issue683_確認記録.md) | STB両公開入口と汎用異厚形状・質量・OVIKAへの結合 | 🔶（IO244成功/2 ignored、App default/gui各2成功、Clippy・文書検証成功。Validation・手動GUI未確認） |
| [STB組立BOX暫定拒否_Issue742_2026-10.md](STB組立BOX暫定拒否_Issue742_2026-10.md) | 参照組立BOXの入力境界と同厚性能保持、GUI読込失敗時の状態保持 | 🔶（変更前40観測の誤受理を再現、IO240成功・Clippy/文書検証成功。GUI default/gui各focused2・既存module11/GUI all-target Clippy成功。Validation・最終ゲート未確認） |
| [STB_CFT鋼管標準密度_Issue741_2026-10.md](STB_CFT鋼管標準密度_Issue741_2026-10.md) | 既知標準鋼材のCFT単独・通常鋼混在の密度と独立面積質量 | 🔶（変更前両公開入口の密度0/質量Err再現、修正後core/IO822成功・共有helper物性4成功・App GUI関連6成功・Clippy/fmt/文書検証成功。Validation・最終ゲート未確認） |
| [弾性梁の有限入力演算_Issue736_2026-10.md](弾性梁の有限入力演算_Issue736_2026-10.md) | 有限極大Gによる可撓部剛性・端部縮約の診断 | 🔶（最新main公開再現・正常対照と既存GVe拒否を区別、レビューの壁側柱先行質量panic修正後element504/solver380・Clippy成功、先行App契約default17/GUI18を区別。最終ゲート・Validation未） |
| [増分部材履歴の有限記録_Issue732_2026-10.md](増分部材履歴の有限記録_Issue732_2026-10.md) | 増分部材履歴の単精度変換と記録不能時の失敗 | 🔶（最新main公開再現、solver全377・App保存契約default16/GUI17成功。原条件の先行Errを区別。最終ゲート・Validation未） |
| [明示仕口パネル採用G_Issue728_2026-10.md](明示仕口パネル採用G_Issue728_2026-10.md) | 明示パネルの採用G・GVe入力拒否と既存自動生成・通常梁拒否の区別 | 🔶（正常対照付き公開入口Verification、core559/element502/job81・solver全371・GUI契約16・Clippy/文書検証成功。途中失敗を区別。最終ゲート・Validation未） |
| [部材荷重微小区間_Issue713_2026-10.md](部材荷重微小区間_Issue713_2026-10.md) | ブレース静定分配・切断荷重・固定端断面力の非零微小幅 | 🔶（公開入口の変更前不一致、荷重込み断面・剛域の追加不一致と修正後element502/solver全369・GUI契約15・Clippy成功（小反力/相殺厳密検査を含む）。文書検査/mdBook成功。最終ゲート・Validation未） |
| [弾性梁採用G_Issue712_2026-10.md](弾性梁採用G_Issue712_2026-10.md) | 採用Gの直接生成・公開静/動解析、SRC/CFT既存拒否との区別 | 🔶（element497・修正後solver368成功。独立レビューの動解析panicは6入口追補で回復、最後の試験変更後focused1・design3・Clippy/文書検証成功。先行App default13/GUI14を区別。最終ゲート・Validation未） |
| [仕口パネル参照柱_Issue585_2026-10.md](仕口パネル参照柱_Issue585_2026-10.md) | 共通解決・生成・解析・検定の参照柱、旧計算契約の失効 | 🔶（直接・関連回帰Verification成功、原典/手動GUI/Validation・最終headゲート未） |
| [壁側柱の物理的分担と局所解放_Issue496_2026-10.md](壁側柱の物理的分担と局所解放_Issue496_2026-10.md) | 共通側柱分類・純鋼板独立K/反力/仕事、Beam/Fiber/MS局所釣合い・軸履歴・同step応答 | 🔶（分離候補のrun別Verification。App lib847・full_model37・壁統合18等のrun別成功と途中失敗を区別。最終ゲート・Validation未確認） |
| [断面JとCFTコアねじり_Issue416_2026-10.md](断面JとCFTコアねじり_Issue416_2026-10.md) | H慣用J・Box中心線式、独立P2参照、本番P1校正144levels、供給Js/実GsとBeam/Fiber・公開solver・OVIKA/結果26/18 | 🔶（数値Verificationと物理Validationを区別。正式Windowsgrid160fresh成功・3形状各3反復を記録。最終統合/公開PRゲートは記録待ち） |
| [STB断面系列別ID_Issue571_2026-10.md](STB断面系列別ID_Issue571_2026-10.md) | 採用namespace・具体系列・数値IDによる定義/部材/強度結合、内部/OVIKA保持、元5Done | 🔶（初期3段階の失敗を保持し、最終feature1322・IO227・Clippy両構成・fmt・文書検査成功。全18 snapshot/1161入力hash一致。XSD124派生期待外0、full_model差分fresh必須指摘なし。文書修正後の再検査/レビュー・親監査・最終ゲート未確認） |
| [STB標準入力限定_Issue660_2026-10.md](STB標準入力限定_Issue660_2026-10.md) | 旧独自入力互換の撤去・固定XSD照合・両公開import参照不正・内部/OVIKA保持 | 🔶（IO200・App GUI lib837・Clippy・文書検査成功、独立レビュー必須指摘なし。追加feature回帰1292成功/1失敗、共有fixture補正後App統合3成功、App試験移行後対象6成功。全体再実行・親最終監査・最終ゲート未確認） |
| [終局需要の重力契約_Issue565_2026-10.md](終局需要の重力契約_Issue565_2026-10.md) | 静的G/Pframe terms・確定増分総応答・RC/CFT不足伝播・鮮度、原5Doneの対応 | 🔶（App実RC/CFT・直接collector・物理柱・旧run混在拒否/回復を確認。default合成3468、最終関連feature1287/io195成功、Clippy/Rustdoc/fmt/文書検証成功。手動GUI・正式ゲート・Validation未確認） |
| [用途別せん断面積と二方向検定_Issue417_2026-10.md](用途別せん断面積と二方向検定_Issue417_2026-10.md) | H/Box/Otherの剛性As・検定Ad、二方向比/τsum、CFT規約差、未算定と検定15資格 | 🔶（最終design623・Windows関連lib1558成功/既存ignore1、Clippy/strictDoc/fmt・文書検査/build成功。Q2前App349/core493/GUI831は別snapshot。正式ゲート・物理Validationは別確認） |
| [SRC梁のQL参照完全性_Issue671_2026-10.md](SRC梁のQL参照完全性_Issue671_2026-10.md) | SRC梁のQL/Q0・分担・位置・QD選択と必要参照、検定版14/解析版23の独立資格 | 🔶（変更後core/design1128成功・App346成功。変更前Sourceと停止前MCP延べ17実行の履歴を分離。MCP専用受入は今回対象外。Windows横断・正式静的/文書検証成功、独立レビュー等の最終ゲート・Validation未確認） |
| [依存監査とOVIKA保存_Issue690_2026-10.md](依存監査とOVIKA保存_Issue690_2026-10.md) | zip脆弱依存・GUI推移的依存の保守終了とOVIKA安全境界 | 🔶（旧8明示/PPMdなしをmetadata確認。最終App441/feature1276成功・監査/静的/文書成功。中間失敗/中断保持、正式ゲート・#671連携は後続） |
| [部材荷重方向の計算入口拒否_Issue687_2026-10.md](部材荷重方向の計算入口拒否_Issue687_2026-10.md) | Point/Distributedの方向境界・有効0/相殺/直交、共通計算入口と入力/成果保持 | 🔶（変更前41ケースと先行MCP9call、変更後core等970・App21＋通知追補4・MCP13試験/20call、保存失効を確認。横断初回2失敗後のApp局所344成功・途中失敗保持。横断1720/関連feature1272・IO191とfeature Clippy/strictDoc成功。最終fmt/default Clippy成功、文書/正式ゲート未確認、native/transport/Validation未実施） |
| [階一括適用API整理_Issue684_2026-10.md](階一括適用API整理_Issue684_2026-10.md) | 旧公開APIの記録寿命、現用App生成・正順Undo/Redo・正常stale保持の直接対照 | 🔶（調査27／恒久16状態を区別。通常665/GUI1151・静的/文書成功、初回失敗保持。正式ゲート未確認） |
| [内部参照検査境界_Issue681_2026-10.md](内部参照検査境界_Issue681_2026-10.md) | 階・拘束・節点荷重・生成記録の存在検査、保存/解析/生成とApp拒否時保持 | 🔶（Source6・App受入/編集9、是正後workspace3422/feature1265/IO191成功。初回失敗保持、静的/最終ゲートは記録時点未確認） |
| [OVIKA参照ID整合性_Issue678_2026-10.md](OVIKA参照ID整合性_Issue678_2026-10.md) | 未完成保存と構造参照拒否、公開断面選択、安定二次ID・失敗時保持 | 🔶（前後41・core/IO・App4件・workspace3414成功、版据置採用。最終ゲート最新状態は[#678](https://github.com/hrntsm/SEPIKA/issues/678)） |
| [RC専用断面の板参照境界_Issue677_2026-10.md](RC専用断面の板参照境界_Issue677_2026-10.md) | RC梁/柱専用3型の床板・壁版参照境界、正常板対照・編集/保存/解析/数量 | 🔶（App正常4/拒否12条件・既存6試験・不正OVIKA27条件・MCP公開JSON3試験default/feature成功。広域・最終ゲートは後続） |
| [RC断面の専用用途_Issue674_2026-10.md](RC断面の専用用途_Issue674_2026-10.md) | RC梁/柱専用用途・逆用途拒否と原子性・正常3型の共有/None・数量/標準STB/OVIKA | 🔶（Edit/OVIKA・正常6条件/MCP4試験Verification成功、広域・正式最終ゲートは後続） |
| [非節点二次部材数量_Issue557_2026-10.md](非節点二次部材数量_Issue557_2026-10.md) | 二次部材の実長・階未確定・SM識別・厳密な実材集約・材料と編集保存 | 🔶（最終直接55・App4/MCP3・編集保存・最終広域default2044/feature1235・両Clippy/strictDoc・文書検査成功。nativeGUI/transport/Validation・最終ゲート未確認） |
| [小梁偏心別の数量集約_Issue792_2026-10.md](小梁偏心別の数量集約_Issue792_2026-10.md) | 偏心一致/相違/未指定・材端反転、RC接触控除・全数量、鋼材独立重量・間柱対照 | 🔶（集約条件Verification成功、施工Validation未実施） |
| [二次部材の共通実材判定_Issue794_2026-10.md](二次部材の共通実材判定_Issue794_2026-10.md) | 偏心・断面・材料と端反転の同一判定、荷重・設計・GUI・所属チェック | 🔶（直接Verification記録、手動GUI・Validation未実施） |
| [RC梁Q0の符号付き端反力合成_Issue600_2026-10.md](RC梁Q0の符号付き端反力合成_Issue600_2026-10.md) | local Qy・端別係数合成・0/不足・公開入口・旧検定失効 | 🔶（Source直接・追加受入8・旧11保存・関連default/feature・両Clippy/strictDoc成功。MCP絶対Q0非公開、手動GUI/Validation・正式レビュー/最終CIは未確認） |
| [単独RC壁Ds応力度と側柱寸法_Issue619_2026-10.md](単独RC壁Ds応力度と側柱寸法_Issue619_2026-10.md) | 柱心間距離によるDs応力度・両側柱寸法・既存評価点／Q・理由付き未算定 | 🔶（独立値・Source焦点Verification成功、広域検証・手動GUI／Validation・最終ゲートは別確認） |
| [鉛直柱支持小梁の自重フェース_Issue423_2026-10.md](鉛直柱支持小梁の自重フェース_Issue423_2026-10.md) | 最外外側面・有限柱閉区間・角丸CftBox・設計DLと芯々断面質量の分離 | 🔶（公開8試験・MassEquiv節点加算・旧Analysis20保存回帰4件・文書検査成功。全形状/手動GUI/Validationと最終ゲートは別確認） |
| [時刻歴時間軸_Issue610_2026-10.md](時刻歴時間軸_Issue610_2026-10.md) | 入力瞬時値・初期釣合い・N−1積分・間引き・checkpoint・既存入口と保存失効 | 🔶（独立fixture・境界・自動GUI/MCP・保存失効・文書Verification成功。手動GUI／Validation・正式ゲートは別確認） |
| [時刻歴の実入力と結果所属_Issue611_2026-10.md](時刻歴の実入力と結果所属_Issue611_2026-10.md) | 立体・質点系の実波形／実効設定・開始対象・保存所属と拒否時状態保持 | 🔶（固定結果・実ファイル本体・保存の受入。モデル条件結合修正後GUI14＋既存617成功、default全体・広域再検査・native操作・Validation・正式ゲートは別確認） |
| [CMQのM成分塗り分割_Issue500_2026-10.md](CMQのM成分塗り分割_Issue500_2026-10.md) | サンプルを保持した塗り分割・独立面積・本番描画・数値と状態の保持 | 🔶（独立算術・自動描画・App広域回帰・両Clippy・strict Rustdoc成功。手動GUI未実施。最終ゲートは[PR #662](https://github.com/hrntsm/SEPIKA/pull/662)） |
| [未変形編集入力_Issue475_2026-10.md](未変形編集入力_Issue475_2026-10.md) | 編集開始直後の未変形pick／snap、既存入口とモデル・履歴・結果の保持 | 🔶（独立算術・自動egui 7件・広域回帰成功。手動GUI未実施。最終ゲートは[PR #661](https://github.com/hrntsm/SEPIKA/pull/661)） |
| [RC壁せん断耐力方式_Issue572_2026-10.md](RC壁せん断耐力方式_Issue572_2026-10.md) | 建物共通min/mean・両consumer・開始時固定・採用run値・保存/Undo/失効・対象材種の方式識別 | 🔶（独立算術/原本照合、最終Windows広域回帰・関連feature・Clippy成功、fresh独立レビュー修正必須0。既存#645除外、Validation未実施。最終ゲートは[PR #656](https://github.com/hrntsm/SEPIKA/pull/656)） |
| [RC壁同一faceと初期耐力_Issue617_2026-10.md](RC壁同一faceと初期耐力_Issue617_2026-10.md) | 同一face全体M/Q/N・物理A・静的一次参照・run採用Quと入力一致 | 🔶（独立算術/人工入力/実求解・App/MCP/Ds/表示とローカルchecks、既知#645除外を区別。独立レビュー指摘なし、Validation未実施。最終ゲートは[PR #655](https://github.com/hrntsm/SEPIKA/pull/655)） |
| [RC壁方向別入力_Issue535_2026-10.md](RC壁方向別入力_Issue535_2026-10.md) | 方向比・材料役割・物理壁Fc/再ID・GUI保存/Undo/失効・MCP開始拒否 | 🔶（入力Verification、手動GUI/実験Validation未） |
| [STB書出の開発中表示_Issue647_2026-10.md](STB書出の開発中表示_Issue647_2026-10.md) | 実ファイルメニューの操作前表示・閉鎖時状態保持・既存入出力回帰 | 🔶（自動描画・既存入出力Verification成功、手動native取消未実施。最終ゲートは[PR #650](https://github.com/hrntsm/SEPIKA/pull/650)） |
| [弧長法削除と既存増分回帰_Issue433_2026-10.md](弧長法削除と既存増分回帰_Issue433_2026-10.md) | 弧長専用実装/API/試験/導線削除、既存荷重増分・変位制御の回帰 | 🔶（限定Verification。最終ゲートは[PR #649](https://github.com/hrntsm/SEPIKA/pull/649)） |
| [SM520告示基準強度と範囲外拒否_Issue629_2026-10.md](SM520告示基準強度と範囲外拒否_Issue629_2026-10.md) | 告示3区分・100 mm超非補完・現役実入口・OVIKA15/6 | 🔶（直接Verification、正式JIS全製品/Validation未確認。最終ゲートは[PR #646](https://github.com/hrntsm/SEPIKA/pull/646)） |
| [質点系先行成果鮮度_Issue612_2026-10.md](質点系先行成果鮮度_Issue612_2026-10.md) | GUI 実依存の存在・入力一致、限定片持柱の独立 K/T、状態保持 | 🔶（自動入口 Verification、手動 GUI・freshレビュー・最終head CI は後続） |
| [リップ溝形の幾何成立条件_Issue544_2026-10.md](リップ溝形の幾何成立条件_Issue544_2026-10.md) | 5矩形境界・独立総性能/質量・生成/保存/編集/GUI/MCP・両質量方式の公開動的Result入口 | 🔶（Windows直接Verification成功、自動App/egui・実MCP・公開solver成功、修正後freshレビュー修正必須なし、手動GUI・実製品Validation・最終docs commit/CIは後続） |
| [主架構線材の2節点入力_Issue562_2026-10.md](主架構線材の2節点入力_Issue562_2026-10.md) | 対象4kindの共通節点数・編集/I/O/数量/共有準備/solver/GUI/MCP入口と状態不変 | 🔶（Windows入口・自動App/egui・MCP Verification成功、手動GUI未実施、最終headレビュー・全CIは後続） |
| [節点TSV一括拒否_Issue615_2026-10.md](節点TSV一括拒否_Issue615_2026-10.md) | 節点TSVの有限値・矩形全体・展開上限・適用時再検査と拒否時状態保持 | 🔶（SEPIKA Pasteイベント経路のVerification。実Excel/OSクリップボードは未実施・必須対象外、最終ゲートは後続） |
| [節点材料の直接有限入力_Issue546_2026-10.md](節点材料の直接有限入力_Issue546_2026-10.md) | 節点・材料直接追加／単一失焦／有限判定と履歴・強度元指定保持 | 🔶（直接egui Verification、review修正後edit202件、既存値修正Undo含む追加eguiは新head CI後続） |
| [矩形床の直交分類_Issue561_2026-10.md](矩形床の直交分類_Issue561_2026-10.md) | 閉合/直交/面積、斜め床真面積・支持辺対応、App適用・OVIKA11/5、#645 Windows境界入力・荷重位置一次 | 🔶（直接幾何Verification、#645の保存入力と実評価を追補。実験Validation・freshレビュー・最終head全CIは後続） |
| [固定距離部材荷重の支間内検証_Issue635_2026-10.md](固定距離部材荷重の支間内検証_Issue635_2026-10.md) | 固定位置・区間の共通拒否／追加更新／座標・コピー／正しいhashの不正OVIKA／GUI・MCP | 🔶（入力境界Verification、freshレビュー・最終head全CIは後続） |
| [取付き版と小梁荷重の支持判定_Issue632_2026-10.md](取付き版と小梁荷重の支持判定_Issue632_2026-10.md) | 取付き床分割・Supported並走小梁・有効割当壁の保持・真未割当STB対照、解析25/検定17 | 🔶（直接81・共通回帰1128・GUI836・既存MCP1とClippy成功。文書／最終ゲート未完了） |
| [主架構支持の解析表現不変性_Issue597_2026-10.md](主架構支持の解析表現不変性_Issue597_2026-10.md) | 明示Beam/Fiber/MSの床壁境界・割当・荷重合力/一次モーメント・編集/Undo/実OVIKA | 🔶（直接Verification、freshレビュー・最終head全CIは後続） |
| [未割当線材のSTB出力拒否_Issue625_2026-10.md](未割当線材のSTB出力拒否_Issue625_2026-10.md) | 両公開export・実Appの未割当拒否／状態保持／明示割当回復、正常5部材の独立属性 | 🔶（直接Verification、固定公式XSD取得403・全文未検証） |
| [外部ID入力拒否_Issue603_2026-10.md](外部ID入力拒否_Issue603_2026-10.md) | MCP/手書き/serde・STB全文書・境界/省略・正常編集/Undo | 🔶（入口Verification、freshレビュー・最終head全CIは後続） |
| [二次CFT入口拒否_Issue539_2026-10.md](二次CFT入口拒否_Issue539_2026-10.md) | 共通用途・raw保存読込・App状態保持・数量/解析入口・正常主柱対照 | 🔶（入口Verification、freshレビュー・最終head全CIは後続） |
| [直接線形求解_Issue598_2026-10.md](直接線形求解_Issue598_2026-10.md) | Cholesky適用・独立f64残差・LU別契約・52,920DOF前後実測・OVIKA旧6失効 | 🔶（Verification、一試行実測、Validation・freshレビュー・最終head全CIは後続） |
| [静的設定採用_Issue595_2026-10.md](静的設定採用_Issue595_2026-10.md) | 公開静的経路・独立スカラー・自然未収束・GUI保存/MCP拒否 | 🔶（Verification、実験Validation・freshレビュー・最終head全CIは後続） |
| [SRC材料領域_Issue552_2026-10.md](SRC材料領域_Issue552_2026-10.md) | 中心直角Hの非重複A/I/EA/EI・実Fiber/MS/MN/Mφ/保存/表示・旧版3 | 🔶（限定幾何Verification、実験Validation・freshレビュー・最終head全CIは後続） |
| [共通断面質量_Issue594_2026-10.md](共通断面質量_Issue594_2026-10.md) | 両質量形式・幾何/採用密度・直接/公開診断・旧契約2成果 | 🔶（ローカルVerification、freshレビュー・最終head全CI後続、Validation未） |
| [標準配筋_Issue507_2026-10.md](標準配筋_Issue507_2026-10.md) | 原配筋・役割/pos/適用、標準出力、OVIKA/編集/解析ゲート | 🔶（製品検証中、原図幾何・最終head全CI未） |
| [標準鋼材名の厳密解決_Issue590_2026-10.md](標準鋼材名の厳密解決_Issue590_2026-10.md) | strict名称・標準物性/F・公開STB未知名/明示物性/native | 🔶（名称Verification、種類別F原典全照合・Validation・最終head全CI未） |
| [大梁横補剛参考数_Issue488_2026-10.md](大梁横補剛参考数_Issue488_2026-10.md) | 材種・H断面・柱間全長/group・支配区間・GUI/MCP参考状態 | 🔶（参考式Verification、配置/性能・Validationは未対応） |
| [フランジ位置の標準形式ゲート_Issue505_2026-10.md](フランジ位置の標準形式ゲート_Issue505_2026-10.md) | 面別上下位置・段違い対象外・旧パネル/solver・SkippedのGUI/MCP/CSV | 🔶（直接Verification、#455実入力・正式段違い式・公開全構成CI・Validation未） |
| [梁パネル寸法_Issue504_2026-10.md](梁パネル寸法_Issue504_2026-10.md) | H/BuiltH幾何・非H未算定・生成/検定/共通準備/manual | 🔶（直接Verification、#505位置/形式は別レポート、最終head全CI・実験未） |
| [床板の同期物理質量_Issue583_2026-10.md](床板の同期物理質量_Issue583_2026-10.md) | 両形状・材料由来physical・同期置換・仕上LL/先端保持・STB優先・層モデル・Undo/鮮度 | 🔶（直接Verification、最終head全CI・freshレビュー後続、実験未） |
| [鋼床板の高密度防護_Issue581_2026-10.md](鋼床板の高密度防護_Issue581_2026-10.md) | 囲み/取付き・境界・設計拒否/LL・STB優先・共通準備/GUI/MCP | 🔶（床板階physical質量は別課題#583、最終headゲート未完） |
| [参照鋼の高密度防護_Issue501_2026-10.md](参照鋼の高密度防護_Issue501_2026-10.md) | 境界/単位・参照対象・設計拒否/物理許可・共通準備/旧Auto失効・GUI/MCP/Undo | 🔶（同コード公開全構成成功、最終headゲート後続。CFTは柱専用、実験Validationは対象外） |
| [標準強度_Issue520_2026-10.md](標準強度_Issue520_2026-10.md) | 標準2.0.2強度の元省略・優先解決・径別/部位別・編集/保存/出力 | ✅（Scope内強度Verification、全相互運用/Validationは未対応） |
| [割当領域同一性_Issue506_2026-10.md](割当領域同一性_Issue506_2026-10.md) | 支持 ID/span・版と入力荷重の保持、孤立版診断、確認付き編集と原子的拒否・Undo/Redo | ✅（Scope内の状態契約Verification。力学・描画Validationは対象外） |
| [地震重量の鮮度と共通準備_Issue439_2026-10.md](地震重量の鮮度と共通準備_Issue439_2026-10.md) | 入力/生成出力一致・GUI/MCP実EX・atomic失敗・Modal世代・Undo/保存・追加/挿入階マスター一意・公開編集ID/Undo | 🔶（入口Verification、#486適用制限と実測Validationは残る） |
| [荷重状態と検定対象_Issue487_2026-10.md](荷重状態と検定対象_Issue487_2026-10.md) | 荷重状態・用途別P・選択重力参照・GUI/MCP・組合せ非保存/単独結果保護・CSV・小梁床長期略算/選択短期未検定 | 🔶（Verification、現行法原文再取得・Validation未） |
| [目的別確定層力_Issue442_2026-10.md](目的別確定層力_Issue442_2026-10.md) | 目的別run／step、符号付き層切断面、外力累積・支持ばね、壁側柱、GUI共通入口・MCP・保存・帳票 | ✅（Scope内Verification。一般配置・実験Validation・現行法全網羅は未完） |
| [RC梁αyと追加端ばね_Issue523_2026-10.md](RC梁αyと追加端ばね_Issue523_2026-10.md) | 採用分岐、T方向別数値、総角と追加角、明示基準の実factory・GUI・解析入口 | 🔶（Scope内Verification。原典完全照合・実験Validation・T全解析接続は未完） |
| [地震力標準式と適用診断_Issue486_2026-10.md](地震力標準式と適用診断_Issue486_2026-10.md) | 標準地震力・Qud/増分標準接続・共通GL/入力/適用診断・GUI/MCP入口と失敗理由 | 🔶（標準式Verification、精算/PH/地域適用等は未対応） |
| [原階所属_Issue497_2026-10.md](原階所属_Issue497_2026-10.md) | 原階保存・編集・Fc供給・公開fixture意味的往復・採用2.0.2全XML schema・GUI/MCP入力世代 | ✅（原階/節点Scope。鋼ブレース文字子はIssue550記録、配筋はIssue507記録で別検証） |
| [鋼ブレースXML文字子_Issue550_2026-10.md](鋼ブレースXML文字子_Issue550_2026-10.md) | 最小鋼ブレース公開export全文の公式2.0.2 XSD成功・文字子negative2件・属性escape・公開importの独立意味照合 | ✅（Scope内Verification。最終ゲートは[PR #648](https://github.com/hrntsm/SEPIKA/pull/648)） |
| [参照骨格の部材角単位_Issue436_2026-10.md](参照骨格の部材角単位_Issue436_2026-10.md) | 正側参考骨格のQ/Ks・明示s/z・不正/未対応拒否・利用側比較 | 廃止済み（#849）。単位契約Verificationの履歴を保持 |
| [耐震壁_未検定_Issue495_2026-10.md](耐震壁_未検定_Issue495_2026-10.md) | 壁版候補・case・種別別未検定とスリット／自重対象外、不正入力、GUI・MCP・CSV・保存 | 🔶（状態契約のVerification。純鋼板国内式・実験Validationは未対応） |
| [壁の確定ステップ応答_Issue443_2026-10.md](壁の確定ステップ応答_Issue443_2026-10.md) | 壁単体Qw/Qdir・基準点付き24成分F/M・実要素N/M・弦変形・確定履歴・GUI/MCP/CSV/保存・設計拒否 | ✅（Scope内Verification。材料γ・壁用イベント・系全体集計・実験Validationは未対応） |
| [リップ溝形材_未検定_Issue509_2026-10.md](リップ溝形材_未検定_Issue509_2026-10.md) | 単一リップ材の3座屈分類、荷重/補剛状態と理由付き未検定、保存・GUI・CSV | 🔶（未検定契約のVerification。耐力式・実験照合は未対応） |
| [明示線材とダンパー総重量_Issue422_2026-10.md](明示線材とダンパー総重量_Issue422_2026-10.md) | 線材重量同等性、実M、重量／配置／動的質量の記録集合の完全性と反映値、公開フレーム・質点系InvalidInput、GUI・undo・保存 | ✅（Scope内 Verification。二次部材支持解決はScope外の残課題） |
| [フィレット_角R_断面整合_Issue413_2026-10.md](フィレット_角R_断面整合_Issue413_2026-10.md) | H・角形 CFT の領域性能、公開耐力・実要素、重量質量、項目別算定元、設計ゲートと公開結果。受入 P2 の Truss 質量源の回帰を含む | ✅ |
| [鋼材名の厳密な強度割増認識_Issue599_2026-10.md](鋼材名の厳密な強度割増認識_Issue599_2026-10.md) | 対応済名称と自動割増・factory/曲げ/せん断・旧解析版7のOVIKA失効 | 🔶（直接Verification、freshレビュー・最終CIは後続） |
| [符号付き単純梁反力_Issue633_2026-10.md](符号付き単純梁反力_Issue633_2026-10.md) | 床／story／elementの符号付き反力・独立積分・利用側・adapter・旧成果失効 | 🔶（直接Verification、原本／Validation・freshレビュー・最終CIは後続） |
| [保存成果の計算契約版_Issue587_2026-10.md](保存成果の計算契約版_Issue587_2026-10.md) | OVIKA解析/検定版・実入力・部分再実行・GUI/CSV・直接床板/小梁 | 🔶（直接Verification、freshレビュー・最終全構成は後続。MCP別ストアは#589） |
| [MCP保存成果の生成元_Issue589_2026-10.md](MCP保存成果の生成元_Issue589_2026-10.md) | 静的/固有値の開始入力・契約版・公開path/hash・stale・保存失敗保持 | 🔶（直接Verification、最終ゲートは[PR #651](https://github.com/hrntsm/SEPIKA/pull/651)） |
| [静的検定の生成入力_Issue602_2026-10.md](静的検定の生成入力_Issue602_2026-10.md) | 同版の検定入力・部分解析非復活・独立0.8→1.6・GUI/CSV/OVIKA/Undo/失敗 | 🔶（直接Verification、freshレビュー・最終全CIは進行担当が確認） |
| [OVIKA_named_MessagePack_2026-10.md](OVIKA_named_MessagePack_2026-10.md) | 4 payload の named 保存・復元、順序独立・default・破損・モデル検証、実 STB モデル単体の codec 時間と最終 OVIKA サイズ | ✅ |
| [壁版の支持範囲判定_2026-09.md](壁版の支持範囲判定_2026-09.md) | 壁の支持辺・間柱端部の負担率の明示入力、部分支持・支持梁重複の診断 | 🔶 |
| [整合質量_2026-09.md](整合質量_2026-09.md) | Beam/Fiber の整合質量、材料領域質量、回転慣性、剛域質量、端部解放の質量縮約 | 🔶 |
| [SRC解析用弾性係数比_Issue434_2026-10.md](SRC解析用弾性係数比_Issue434_2026-10.md) | SRCの解析比・許容曲げ比の分離、材料不正の理由付き停止、独立数値と読込み/編集/準備入口（旧フォールバック記録は履歴） | 🔶（軸・曲げと入口検証済み、原本全式/実験未） |
| [NewRC高強度コンクリート外挿_2026-09.md](NewRC高強度コンクリート外挿_2026-09.md) | Issue #385: Fc>60 のコンクリートを NewRC 式で外挿する現行仕様 | 🔶 |

### 計算機能の検証記録

| レポート | 対象 | 判定 | 備考 |
|----------|--------|------|------|
| [load_calculation_review.md](load_calculation_review.md) | 荷重計算 | 🔶 | 風荷重等のギャップ |
| [剛性計算の実装確認記録](剛性計算_参照実装照合.md) | 剛性計算の採用実装・修正記録 | 🔶（規準本体・独立根拠との照合未完） | 免震・製品要素は未実装 |
| [応力解析の実装・検証記録](応力解析_参照実装照合.md) | 応力解析の実装・試験対応 | 🔶（方式・係数の全面的な規準適合は未確認） | 制振間柱等 |
| [断面検定（許容応力度検定）の実装確認記録](断面検定_参照実装照合.md) | 断面検定の採用実装・修正記録 | 🔶（許容値・接合部係数等の根拠照合未完） | C 節に残置項目 |
| [非線形モデルの実装・検証記録](非線形モデル_参照実装照合.md) | 非線形モデルの実装・試験履歴 | 🔶（係数・適用条件の一次資料照合未完） | 免震 UI・分割ロジック等 |
| [終局検定の実装・検証記録](終局検定_参照実装照合.md) | 終局検定、§6-c 正方形CFT長柱Case2（#325） | 🔶（一次画像/独立期待値、Source式・core・共有静的符号是正job、修正後App/MCP6件の成功証拠を受領。RC影響あり。手動GUI/外部transport・規準本文・Validation未確認、広検証/正式レビューは後続） | #565他残契約は未完。Vu・二軸曲げ等 |
| [ゼロ増分履歴保持_Issue596_2026-10.md](ゼロ増分履歴保持_Issue596_2026-10.md) | 同点trial/probe/commitの全履歴・独立応答、実集中ばね/非対称壁/1D・spatial時刻歴、解析5の実OVIKA失効と明示回復 | 🔶（代表入口Verification、実験Validation未） |
| [非線形動的解析の実装・検証記録](非線形動的解析_参照実装照合.md) | 非線形動的解析の実装・挙動テスト | 🔶（規準本文・装置評定の適合は未確認） | UI/IO 経路整備 |
| [梁型枠接触和集合_Issue451_2026-10.md](梁型枠接触和集合_Issue451_2026-10.md) | 水平一定矩形梁の左右別接触和集合・公開数量入口。実施工Validationは未実施 | 🔶 |
| [数量積算の実装・検証記録](数量積算_参照実装照合.md) | 数量積算の実装・試験履歴 | 🔶（採用値・仮定の妥当性確認未完） | モデル制約による C 節 |
| [RC壁方向別数量_Issue536_2026-10.md](RC壁方向別数量_Issue536_2026-10.md) | 明示方向筋・不完全小計・共有数量設定保存と解析入力境界 | 🔶（独立算術・自動実入口・Windows広域回帰成功、Validation未実施。最終ゲートは[PR #659](https://github.com/hrntsm/SEPIKA/pull/659)） |

### 原典・論文・資料照合

| レポート | 対象 | 判定 |
|----------|------|------|
| [原典照合リスト.md](原典照合リスト.md) | 法令・規準の埋め込み値の照合チェックリスト（実装者は使わない） | 運用中 |
| [材料強度_基準強度照合.md](材料強度_基準強度照合.md) | 告示・材料強度資料 | 🔶 |
| [未入力材料強度の危険側フォールバック_2026-07.md](未入力材料強度の危険側フォールバック_2026-07.md) | フォールバック是正 | 🔶 |
| [ファイバー材料モデル_論文照合_2026-07.md](ファイバー材料モデル_論文照合_2026-07.md) | MP/Yassin、および削除済みManderの歴史的照合記録 | ✅ |
| [ファイバー形状_長期初期載荷_2026-07.md](ファイバー形状_長期初期載荷_2026-07.md) | ファイバー形状・長期荷重 | ✅ |
| [仕口パネル_定式化と検証_2026-07.md](仕口パネル_定式化と検証_2026-07.md) | パネルゾーン力学 | 🔶 |
| [床荷重XY両方向分配_格子方式比較_2026-09.md](床荷重XY両方向分配_格子方式比較_2026-09.md) | 2026-09時点の矩形/非矩形200×200比較記録。U/Cの現行VerificationはIssue437記録、台形の追加精度・原因分解はIssue438記録 | 🔶 |
| [凸台形の独立幾何と格子差_Issue438_2026-10.md](凸台形の独立幾何と格子差_Issue438_2026-10.md) | 固定凸台形の独立半平面幾何、旧1.99%差の全辺原因分解、本番全64格子/順序条件＋比較側8条件、境界実面積・率和・辺別上界と荷重保存 | ✅（Scope内Verification、実験Validation未） |
| [凹形床の有限線分と格子積分_Issue437_2026-10.md](凹形床の有限線分と格子積分_Issue437_2026-10.md) | U/Cと凸台形の独立解析辺面積、境界真面積、共有端点群、格子寸法/位相、回転/順序、実入口と異常/資源/精度診断。幾何Verification、実験Validationと通常結果への診断出力#555は未実施 | 🔶 |

### 敵対的レビュー・定式化レビュー

| レポート | 対象 | 判定 |
|----------|------|------|
| [adversarial_review_2026-07.md](adversarial_review_2026-07.md) | 横断（PO・剛域・MITC4 等） | 🔶 |
| [解析コア_敵対的レビュー_2026-07.md](解析コア_敵対的レビュー_2026-07.md) | 静解析・固有値・増分解析 | 🔶 |
| [壁断面統一と正負別せん断耐力_2026-09.md](壁断面統一と正負別せん断耐力_2026-09.md) | 材料別実幅・SRC/CFT・任意角度・正負別壁ばね、PR #277統合後の回帰結果 | 🔶（数値検証済み・実験照合未） |
| [壁横筋入力と終局割線骨格_Issue503_2026-10.md](壁横筋入力と終局割線骨格_Issue503_2026-10.md) | 明示横筋入力・成分別不足診断・終局割線接続のVerification、新規実験Validation未実施 | 🔶 |
| [RC壁ひび割れ耐力_Issue534_2026-10.md](RC壁ひび割れ耐力_Issue534_2026-10.md) | 無開口Qcの式・壁板面積・独立算術、直接一括／GUI／公開MCP・保存失効のVerification | 🔶（Scope内Verification成功、実験Validation未実施。最終ゲートは[PR #658](https://github.com/hrntsm/SEPIKA/pull/658)） |
| [耐震壁_敵対的レビュー_2026-07.md](耐震壁_敵対的レビュー_2026-07.md) | 壁エレメントの再現試験・独立算式照合・追加回帰検証（判断と途中履歴はADR・申し送りを参照） | 🔶 |
| [保有水平耐力_プッシュオーバー_敵対的レビュー_2026-07.md](保有水平耐力_プッシュオーバー_敵対的レビュー_2026-07.md) | ルート3 PO | 🔶 |
| [材端集中ばね梁_定式化レビュー_2026-07.md](材端集中ばね梁_定式化レビュー_2026-07.md) | ConcentratedSpringBeam | 🔶 |
| [増分解析_ヒンジ形成と剛性低下_検証_2026-07.md](増分解析_ヒンジ形成と剛性低下_検証_2026-07.md) | ヒンジ・剛性低下 | ✅ |
| [増分解析_長期載荷の接線剛性破綻_2026-07.md](増分解析_長期載荷の接線剛性破綻_2026-07.md) | 長期荷重接線剛性 | ✅ |
| [変位制御_荷重パターン保持_2026-07.md](変位制御_荷重パターン保持_2026-07.md) | 変位制御 PO | ✅ |
| [剛域_保有水平耐力_系レベル検証.md](剛域_保有水平耐力_系レベル検証.md) | 剛域の系レベル影響 | 🔶 |
| [小梁設計のスラブ帰属_レベル一致_2026-08.md](小梁設計のスラブ帰属_レベル一致_2026-08.md) | 小梁が負担するスラブの決め方 | 🔶 |
| [床辺荷重の梁への幾何割付_2026-08.md](床辺荷重の梁への幾何割付_2026-08.md) | 床の辺荷重を覆う梁へ割り付ける経路 | ✅ |
| [床領域のパネル統合_2026-08.md](床領域のパネル統合_2026-08.md) | ST-Bridge 小片スラブを大梁の床領域へ畳む | 🔶 |
| [床領域の再設計_荷重分配とSlabFloorRegion分離_2026-08.md](床領域の再設計_荷重分配とSlabFloorRegion分離_2026-08.md) | 荷重分配を `FloorRegion`/`Slab` 単位へ作り替え、型を再分離 | 🔶 |
| [小梁検定の負担幅を床板境界から求める_2026-08.md](小梁検定の負担幅を床板境界から求める_2026-08.md) | 小梁検定の負担幅を床板境界の幾何から求める（Step 5 一部。**二次部材経路は §5.39・§5.40 で supersede**） | 🔶 |
| [小梁設計を分配結果から出す_2026-08.md](小梁設計を分配結果から出す_2026-08.md) | 二次部材小梁の断面検定を分配 `Span` から出す（Step 5。§5.40〜§5.43） | ☑ |
| [二次部材の反力の逐次伝達_2026-08.md](二次部材の反力の逐次伝達_2026-08.md) | 小梁を二次部材へ一本化し、二次部材に支持された二次部材の荷重が解析から消える危険側の穴を塞ぐ（§3.4・§5.44）。実データで床固定荷重の 7.2% が失われていた | ☑ |
| [要素にならない壁版の可視化と耐震スリット_2026-09.md](要素にならない壁版の可視化と耐震スリット_2026-09.md) | 要素にならない壁版を 3D ビュー・モデル化図へ描く（§5.46）。耐震スリットを辺ごとの入力（`WallSlit`）へ一般化し、垂れ壁付き梁の剛性を無視していた危険側を是正（§5.47）。既存の代表スカラは全項目不変 | 🔶 |
| [壁版から間柱への荷重分配と雑壁の壁版参照化_2026-09.md](壁版から間柱への荷重分配と雑壁の壁版参照化_2026-09.md) | 壁エレメントを「壁領域全体を覆う 4 節点の壁版」に限定し、要素にならない壁版の自重を一方向版として辺へ配る（§3.5・§5.45）。フレーム内雑壁の幾何を壁版直接参照へ移した。既存の代表スカラは全項目不変 | 🔶 |
| [フレーム外雑壁の壁版への移行_2026-09.md](フレーム外雑壁の壁版への移行_2026-09.md) | `OutOfFrameMiscWall` を廃止し取り付く壁版へ吸収（§5.48）。壁の仕上げ・増打ちの面荷重、立ち上がり高さの未指定（＝階高）、n 倍法の自立壁限定を含む。**旧「柱」伝達が階の地震用重量から壁の重量の 49.9% を落としていたことを実測** | 🔶 |
| [壁版の取り込み・要素生成・参照張り替え_2026-08.md](壁版の取り込み・要素生成・参照張り替え_2026-08.md) | 壁の解析要素を準備計算からの生成物へ転換（Step 7+8 本体） | ☑ |
| [剛域算定の壁展開順序不整合_2026-08.md](剛域算定の壁展開順序不整合_2026-08.md) | 壁展開モデルを見ていなかった4箇所（剛域自動算定・耐震壁のせん断断面検定・数量拾い・保有水平耐力の部材ランク自動判定）の是正 | ☑ |
| [RC鉄筋比契約_Issue435_2026-10.md](RC鉄筋比契約_Issue435_2026-10.md) | RC梁のαy小数/bD、せん断百分率/bd、T形方向別定義と入力不足停止 | 🔶（単位契約のVerification済み、T形解析接続・原著・Validation未完） |

### フェーズ監査

> フェーズは #313 で廃止した `dev_docs/specs/` のフェーズ実装仕様書の識別子（旧フェーズ）
> であり、現在の仕様・計画を指すものではない。

| レポート | 旧フェーズ | 判定 |
|----------|----------|------|
| [p3_review.md](p3_review.md) | P3 最小 UI | 🔶 |
| [p4_review.md](p4_review.md) | P4 材料断面 | ✅ |
| [p7_review.md](p7_review.md) | P7 二次設計 | 🔶 |
| [Fes 算定不能の伝播](Fes算定不能の伝播_Issue718_2026-10.md) | Issue718・通常式/実集計/算定不能の自動 GUI・CSV・保存・再計算 | 🔶 |
| [p8_review.md](p8_review.md) | P8 操作連携 | 🔶 |
| [pending_items.md](pending_items.md) | P9 仕上げ | 🔶 |

## 索引（要素/設計式 → テスト）

傾斜柱のMSなし仕口パネル検定は[Issue764の調査記録](傾斜柱仕口パネル_Issue764_調査記録.md)を参照する（🔶）。一時試験による公開入口の未算定再現と独立自由体の算術を記録し、恒久回帰・一般荷重pMの確定は未実施。

> 「旧フェーズ」は #313 で廃止した `dev_docs/specs/` のフェーズ実装仕様書の識別子であり、
> 現在の仕様・計画を指すものではない。

| # | 対象 | クレート | ソースファイル | テスト関数 | 旧フェーズ | 状態 |
|---|------|----------|---------------|-----------|---------|------|
| 731 | 梁の床板嵩上げ | sepika-core / sepika-load / sepika-job / sepika-app | beam_placement.rs・self_weight/beam_upstand.rs・cascade.rs・app/girder_weight_tests.rs | [独立値と実入口](梁の床板嵩上げ_Issue731_2026-10.md#確認した入口) | 横断 | 🔶（数値Verification、手動GUI・Validation未） |
| 810 | 層指標の生成入力照合 | sepika-app | summary.rs・design_view.rs | [独立期待値と実入口](層指標の生成入力照合_Issue810_2026-10.md#対象と独立期待値) | 横断 | 🔶（手動GUI・Validation未） |
| 808 | 塑性確定点のバイリニア接線 | sepika-material / sepika-element / sepika-solver | bilinear_yield_surface.rs・frame/concentrated・general_member_loads.rs | [自動回帰と実行証拠](塑性確定点のバイリニア接線_Issue808_2026-10.md#自動回帰と実行証拠) | 横断 | 🔶（方向別Verification、減衰用接線の判断未決） |
| 802 | 集中ばね梁の部材荷重 | sepika-element / sepika-solver / sepika-mcp | frame/concentrated・general_member_loads.rs・persist.rs | [独立期待値と公開入口](集中ばね梁の部材荷重_Issue802_2026-10.md#対象と独立期待値) | 横断 | 🔶（限定Verification、Validation未） |
| 616 | 大梁自重の床板実重複控除 | sepika-load / sepika-job / sepika-app | self_weight/girder_overlap.rs・app/girder_weight_tests.rs・weight_preparation.rs | [独立値と実入口](大梁床板実重複控除_Issue616_確認記録.md#独立期待値と実入口) | 横断 | 🔶（数値Verification、Validation未） |
| 775 | ctutils 推移依存監査 | Cargo.lock / sepika-io | zip、ctutils、OVIKA | [監査と実行証拠](ctutils推移依存監査_Issue775_2026-10.md#実行証拠) | 横断 | 🔶（0.4.5 の最小解決、監査・IO default/parquet 成功。独立レビュー・最終 CI は記録を参照） |
| 496 | 壁側柱の物理的分担と局所解放 | sepika-core / sepika-element / sepika-solver / sepika-design-jp / sepika-job / sepika-app | model/wall_side_column.rs、wall/side_column、frame/fiber、tests/wall_system_linear.rs | [原七Doneとrun別結果](壁側柱の物理的分担と局所解放_Issue496_2026-10.md#完了条件と試験の対応) | 横断 | 🔶（限定Verification、full_model3snapshot未受入・最終ゲート・Validation未確認） |
| 671 | SRC梁のQL参照完全性 | sepika-design-jp / sepika-core / sepika-app | srrc/tests.rs、member_design_check.rs、result_contract.rs、app/src_ql_acceptance_tests.rs | [変更後の直接検証](SRC梁のQL参照完全性_Issue671_2026-10.md#変更後の直接検証)・[Appと保存受入](SRC梁のQL参照完全性_Issue671_2026-10.md#変更後のappと保存受入) | 横断 | 🔶（直接/公開actions/OVIKA資格・Windows横断Verification成功、MCP専用受入は今回対象外、最終ゲート・Validation未確認） |
| 690 | 依存監査とOVIKA保存 | workspace / sepika-io / sepika-app | Cargo.toml、Cargo.lock、deny.toml、ovika.rs | [実使用APIと維持する境界](依存監査とOVIKA保存_Issue690_2026-10.md#実使用apiと維持する境界)・[実行証拠](依存監査とOVIKA保存_Issue690_2026-10.md#実行証拠) | 横断 | 🔶（最終旧8構成App441/関連feature1276成功・監査/静的/文書成功。中間IO195を区別、正式review/最終CI・#671連携は後続） |
| 687 | 部材荷重方向の計算入口拒否 | sepika-core / sepika-solver / sepika-job / sepika-app / sepika-mcp | model/load.rs、statics/analysis/tests.rs、job/tests/member_load_direction.rs、member_load_direction_acceptance_tests.rs、mcp/tests/member_load_direction.rs、persist.rs | [直接回帰](部材荷重方向の計算入口拒否_Issue687_2026-10.md#変更後の共通入口と直接回帰)・[App](部材荷重方向の計算入口拒否_Issue687_2026-10.md#変更後のapp公開actionundoovika)・[MCP](部材荷重方向の計算入口拒否_Issue687_2026-10.md#変更後のmcp公開job保存成果) | 横断 | 🔶（方向拒否・正常値・保持/Undo/保存と旧成果失効を確認。構成/追補の重複を独立件数へ足さない。横断1720/feature1272・IO191を記録（記法修正前snapshot）。最終fmt/default Clippy成功、文書/正式ゲート未確認） |
| 684 | 階一括適用API整理 | sepika-edit / sepika-app / sepika-job | testsのapply_prepared_model、tests/story_application_lifecycle・source_story、actions/loads、weight_preparation | [直接観測結果](階一括適用API整理_Issue684_2026-10.md#直接観測結果)・[是正後の恒久回帰と直接検証](階一括適用API整理_Issue684_2026-10.md#是正後の恒久回帰と直接検証) | 横断 | 🔶（旧API廃止、通常665/GUI1151・静的/文書成功。初回失敗保持。正式ゲート/手動GUI/外部利用未確認） |
| 681 | 内部参照検査境界 | sepika-core / sepika-io / sepika-load / sepika-solver / sepika-app / sepika-edit | model/aggregate、story_gen/generate、tests/reference_boundaries・ovika_reference_boundaries・generated_master_references、app/reference_identity_acceptance_tests | [恒久回帰と独立期待契約](内部参照検査境界_Issue681_2026-10.md#恒久回帰と独立期待契約)・[App拒否と保持](内部参照検査境界_Issue681_2026-10.md#app-の拒否と保持) | 横断 | 🔶（是正後広域成功と初回失敗を区別。1.99.1初期化構文是正後focused6/対象Clippy成功。全体静的/最終ゲートは記録時点未確認、手動GUI/Validation未） |
| 734 | 梁の鉛直偏心接続 | sepika-core / sepika-element / sepika-solver / sepika-app | factory/tests、frame/eccentric、statics/linear/tests、common_section_mass、app/beam_placement_tests | [独立期待値](梁鉛直偏心接続_Issue734_確認記録.md#導出と独立期待値) | 横断 | 🔶（数値Verification、手動GUI/Validationは未実施） |
| 726 | 梁の一律鉛直偏心と有限実配置 | sepika-core / sepika-edit / sepika-app / sepika-io | model/beam_placement、app/beam_placement_tests、viewer/beam_placement_tests | [独立期待値と直接検証](梁鉛直偏心_Issue726_確認記録.md#独立期待値と直接検証) | 横断 | 🔶（直接Verification、偏心剛性・重量算定・手動GUI/Validationは対象外） |
| 677 | RC専用断面の板参照境界 | sepika-core / sepika-edit / sepika-io / sepika-solver / sepika-app / sepika-mcp | model/section・aggregate、refs・wall_misc、ovika、tests/rc_section_purpose・rc_plate_section_purpose | [独立期待値と実入口](RC専用断面の板参照境界_Issue677_2026-10.md#修正後の独立期待値と実入口) | 横断 | 🔶（App/IO・MCP公開JSON3試験成功、広域・最終ゲート後続） |
| 678 | OVIKA参照ID整合性 | sepika-core / sepika-io / sepika-app | model/aggregate・section・tests、ovika_reference_tests、actions/io・reference_identity_acceptance_tests | [前後実観測と恒久回帰](OVIKA参照ID整合性_Issue678_2026-10.md#修正前後の実観測) | 横断 | 🔶（前後41・App4件・workspace3414成功。記録時点と[#678の最終ゲート](https://github.com/hrntsm/SEPIKA/issues/678)を区別） |
| 674 | RC断面の専用用途 | sepika-core / sepika-edit / sepika-io / sepika-solver / sepika-app / sepika-mcp | model/section・aggregate、ovika、tests/rc_section_purpose | [独立期待値と実入口](RC断面の専用用途_Issue674_2026-10.md#独立期待値と実入口) | 横断 | 🔶（Edit/OVIKA・正常6条件/MCP4試験成功、広域・最終ゲート後続） |
| 619 | 単独RC壁のDs応力度と側柱寸法 | sepika-design-jp / sepika-app / sepika-core | secondary/ds_group.rs・actions/design/holding.rs・result_contract.rs | [独立期待値と実入口](単独RC壁Ds応力度と側柱寸法_Issue619_2026-10.md#独立期待値と試験への対応) | 横断 | 🔶（Source焦点Verification成功、広域・Validation・最終ゲート別確認） |
| 28a | 鉛直Column支持の小梁自重 | sepika-core / sepika-edit / sepika-load | face_distance.rs, tests/column_support_faces.rs | [Issue423の独立値・公開入口・全5Done](鉛直柱支持小梁の自重フェース_Issue423_2026-10.md) | 横断 | 🔶（公開8試験/MassEquiv節点加算/旧20保存回帰Verification、Validation未実施） |
| 1d | 質点系先行成果鮮度 | sepika-app | app/actions/lumped_mass.rs | [Issue612検証](質点系先行成果鮮度_Issue612_2026-10.md)：実成果存在・入力一致、限定片持柱独立 K/T、拒否/失敗/取消保持 | P1/P2 | 🔶（自動入口 Verification、手動 GUI 未） |
| 618 | RC梁の左右床板厚 | sepika-element / sepika-core | frame/beam/{stiffness_factors,construct,tests}.rs、result_contract.rs | [独立期待値とテスト索引](RC梁左右床板厚_Issue618_2026-10.md#独立期待値とテスト索引) | 幾何剛性 | 🔶（有限例Verification） |
| 786 | RC片持ち架構の階入力と安定性 | sepika-app / sepika-element / sepika-solver | app/tests.rs | [独立モード・変位・支持釣合いと自動回帰](RC片持ち架構の階入力_Issue786_確認記録.md#自動回帰と実行記録) | 横断 | 🔶（有限Verification、手動GUI・物理Validation未） |
| 1 | ティモシェンコ梁 | sepika-element | beam.rs | `test_phi_zero_converges_to_bernoulli`（Bernoulli 極限。軸 EA/L・ねじり GJ/L を含む 12×12 成分。テスト再編により旧 `test_beam_axial_stiffness`・`test_beam_torsion_stiffness` を統合）, `test_torsion_not_stiffened_by_rigid_zone`（剛域ありでもねじりは GJ/L のまま） | P1 | ✅ |
| 760 | 梁文書の局所軸と保存性能 | sepika-core / sepika-element / sepika-app | section_shape/{properties,builder}.rs、geom/vec3.rs、frame/beam/{construct,stiffness,tests}.rs、app/shear_area_acceptance_tests.rs | [独立座標図・手計算と直接試験](梁要素の局所軸と保存性能_Issue760_2026-10.md#独立手計算と直接試験) | 文書 | ✅（座標・添字の対応のVerification） |
| 713 | 部材荷重の微小区間 | sepika-element / sepika-solver | frame/member_load.rs・common/linear_load.rs・statics/linear | [独立期待値と公開入口](部材荷重微小区間_Issue713_2026-10.md#独立期待値と公開入口) | 横断 | 🔶（限定Verification、後続ゲート・Validation未） |
| 1g | 共通局所座標の断面参照方向 | sepika-core / sepika-element / sepika-solver / sepika-job / sepika-design-jp / sepika-app | geom/vec3.rs・face_distance.rs・model/element.rs・transform.rs・statics/analysis/precheck.rs・prepare.rs・secondary/eccentricity・lumped_mass.rs・common/assemble.rs・dynamic/eigen/mod.rs・app/actions/design/check.rs・app/result_contract_tests.rs | [Issue762の修正前再現と直接検証](共通局所座標の参照方向_Issue762_2026-10.md#修正後の直接verification) | 横断 | 🔶（公開拒否と正常変換、対象RC Q0 8件・限定helper後App GUI lib880件、旧直接23失効・必要支持材/柱拒否と未使用梁保持。先行878件・880件、当時の#769未対応・Validation・最終ゲートは記録を参照） |
| 1h | 支持材フェースの有限参照方向 | sepika-core / sepika-load / sepika-app | geom/vec3.rs・face_distance.rs・cascade.rs・app/actions/design/check.rs・app/result_contract_tests.rs | [Issue769の公開支持面と需要の検証](支持材フェースの有限参照方向_Issue769_2026-10.md) | 横断 | 🔶（現main公開支持面の赤再現、先行core/load・App GUI880件と後続平行丸め境界core584・App契約29件、旧38/24失効のVerification。最終全件・Validation・最終ゲートは記録を参照） |
| 1i | 支持面と共通直交基底 | sepika-core / sepika-load / sepika-app | geom/vec3.rs・face_distance.rs・cascade.rs・app/actions/design/check.rs・app/result_contract_tests.rs | [Issue772の専用target独立Verification](支持材フェースと共通直交基底_Issue772_2026-10.md#専用targetでの独立verification) | 横断 | 🔶（公開赤、専用targetの面12/core585・荷重/App焦点・旧39/25失効。共有targetの別試験集合と最終ゲート境界は記録を参照） |
| 1e | 弾性梁の採用G入力検証 | sepika-element / sepika-solver | frame/beam/construct.rs, factory/input_check.rs, statics/linear/tests.rs | [Issue712の公開入口と正常対照](弾性梁採用G_Issue712_2026-10.md) | 横断 | 🔶 |
| 1f | 弾性梁の有限入力演算 | sepika-element / sepika-solver | frame/beam/construct.rs・stiffness.rs・behavior.rs, factory/input_check/tests.rs, tests/panel_zone.rs | [Issue736の公開再現と理由付き診断](弾性梁の有限入力演算_Issue736_2026-10.md) | 横断 | 🔶（限定Verification、最終ゲート・Validation未） |
| 1b | SRC解析用弾性係数比 | sepika-core / sepika-element / sepika-solver / sepika-app / sepika-design-jp | section_shape/composite.rs, frame/beam/stiffness_factors.rs, app/preparation.rs | `src_analysis_ratio_matches_independent_rectangular_h_section` / `src_loaded_and_edited_materials_never_use_provisional_analysis_properties` / `src_invalid_ec_and_poisson_are_analysis_errors` / `allowable_bending_ratio_boundaries_are_separate_from_analysis_ratio` | 横断 | 🔶（原本全式/実験未） |
| 1c | H/BoxのJ・CFTコアねじり | sepika-core / sepika-element / sepika-solver / sepika-io / sepika-app / sepika-mcp | section_shape/core_torsion、tests/cft_torsion_contract.rs、frame/beam/torsion_contract_tests.rs、tests/torsion_contract.rs、tests/cft_torsion_roundtrip.rs | [元6Doneと数値・実入口の証拠](断面JとCFTコアねじり_Issue416_2026-10.md) | 横断 | 🔶（独立数値Verification、正式grid成功・最終ゲート記録待ち・物理Validation未） |
| 1a | 線材の整合質量 | sepika-core / sepika-element / sepika-solver | model/mass.rs, frame/prismatic.rs, frame/rigid_arm.rs, beam/behavior.rs, fiber/mod.rs, frame/truss.rs, dynamic/eigen/tests.rs | 材料領域入口・共通実装・`rigid_zone_mass`・`transform_mass`・Beam/Fiber/Truss 質量入口・座標変換。Truss の受入 P2 回帰は [Issue #413 検証](フィレット_角R_断面整合_Issue413_2026-10.md)。Fiber の非線形 factory 生成は確認済みだが、solver の固有値組立は線形 `BeamElement` 経路であり、Fiber 固有値は未検証。対応テストと条件は [整合質量_2026-09.md](整合質量_2026-09.md) の追跡表を正とする | P1/P2 | 🔶（Kbb 特異時の失敗経路・Fiber 固有値未検証を含む。Truss P2 回帰は検証済み） |
| 1c | 共通断面質量（両行列形式） | sepika-core / sepika-element / sepika-solver / sepika-app | mass.rs・beam/fiber・assemble.rs・result_contract.rs | [Issue594独立oracleと入口](共通断面質量_Issue594_2026-10.md) | 横断 | 🔶（Verification、全CI・freshレビュー後続、Validation未） |
| 1b | 明示線材自重・ダンパー総重量 | sepika-core / sepika-load / sepika-solver / sepika-job / sepika-edit / sepika-app / sepika-io | story_gen/tests.rs, tests/weight_mass.rs, tests/damper_mass_errors.rs, dynamic/lumped_mass/model.rs・job/lumped_mass.rs, load_case.rs・edit/tests.rs, app/tests.rs, model/tests.rs, ovika.rs | [Issue #422 検証](明示線材とダンパー総重量_Issue422_2026-10.md) のテスト対応表。実M・両方式・重量／配置・ゼロ追加・公開フレーム／質点系入口・undo／保存を含む | P1/P2 | ✅（Verification） |
| 2 | 剛域あり梁 | sepika-element | beam.rs | `test_apply_auto_rigid_zones_and_manual_protection`, `test_auto_rigid_zone_only_when_all_members_are_rc`（自動剛域の適用と手動端の保護・適用条件。テスト再編により旧 `test_auto_rigid_zone_standard_formula` を統合） | P1 | 🔶 |
| 3 | 端部ばね（ピン・半剛） | sepika-element | beam.rs | `test_pinned_end_rotation_stiffness_exactly_zero`（テスト再編により旧 `test_pinned_end_releases_moment` を統合） | P1 | 🔶 |
| 4 | MITC4 シェル（膜） | sepika-element | shell.rs | `test_patch_membrane_distorted`（歪みメッシュ・機械精度） | P1.5 | ✅ |
| 5 | MITC4 シェル（曲げ） | sepika-element | shell.rs | `test_patch_bending_distorted`（歪みメッシュ定曲率・機械精度） | P1.5 | ✅ |
| 6 | MITC4 シェル（せん断/収束） | sepika-solver | linear.rs | `test_plate_convergence`（単純支持・四辺固定の板たわみ ±2% 収束＝ロッキングなし。テスト再編により旧 2 テストを統合） | P1.5 | ✅ |
| 723 | S/CFT仕口パネル耐力式の原典対応 | sepika-element / sepika-design-jp | springs/panel.rs / joint_wiring/steel_panel.rs / steel/panel_zone.rs | [原画像と現行式の照合](仕口パネル耐力式_Issue723_原典調査.md#vvと要素証拠索引)、新規数値試験なし | 原典 | 🔶（N/A/F・式の採否未確定、Validation未） |
| 7 | パネルゾーンのフェイスモーメント | sepika-element | panel.rs | `test_face_moments_reference_case1`（pQc=851.135kN 等）, `test_face_moments_reference_case2_t_joint`（ト型） | P1 | ✅ |
| 7a | 仕口パネル（せん断変形角の追加自由度） | sepika-solver | tests/panel_zone.rs | `test_panel_shear_angle_matches_closed_form`（M=K·γ）, `test_panel_dof_equilibrium_residual_is_zero`（釣合い残差がゼロ）。定式化と残課題は [仕口パネル_定式化と検証_2026-07.md](仕口パネル_定式化と検証_2026-07.md) | P1 | ✅ |
| 7b | 梁パネル寸法と未算定伝播 | sepika-core / sepika-element / sepika-design-jp / sepika-job / sepika-solver | panel_zone.rs / springs/panel_gen.rs / joint_wiring/steel_panel.rs / prepare.rs | [Issue504独立期待値・実入口](梁パネル寸法_Issue504_2026-10.md) | 横断 | 🔶（位置/形式は#7c、最終head全CI・実験未） |
| 7l | 仕口パネルの方向別接合面距離 | sepika-core / sepika-element / sepika-job / sepika-solver / sepika-app | panel_zone.rs / panel_gen.rs / prepare.rs / modeling.rs / result_contract_tests.rs | [Issue759の公開再現・独立距離・釣合いと仕事](仕口パネル接合面距離_Issue759_2026-10.md) | 横断 | 🔶（Verification、Validation未） |
| 7k | 仕口パネルの実軸力比 | sepika-element / sepika-solver / sepika-core / sepika-app | springs/panel.rs / tests/panel_zone.rs / result_contract.rs / result_contract_tests.rs | [Issue761の修正前実測と独立期待値](仕口パネル実軸力比_Issue761_2026-10.md) | 横断 | 🔶（限定Verification、Validation・最終ゲート未） |
| 7j | 仕口パネル検定の柱2軸集約 | sepika-design-jp / sepika-core / sepika-app | joint_wiring/steel_panel.rs / material_grade.rs / result_contract_tests.rs | [Issue758の独立需要・公開検定・未対応域](仕口パネル検定2軸集約_Issue758_2026-10.md) | 横断 | 🔶（極小nz修正後の直接11/design665・App保存3、先行App全475・関連ClippyのVerification。原典適合・Validation・最終ゲートは記録を参照） |
| 7i | 仕口パネルの柱2軸と座標 | sepika-core / sepika-element / sepika-solver / sepika-job / sepika-app | panel_zone.rs / springs/panel.rs / frame/panel_offset.rs / prepare.rs / result_contract_tests.rs | [Issue757の実諸元・公開解析・保存と制約](仕口パネル柱2軸_Issue757_2026-10.md) | 横断 | 🔶（方向別諸元/F・3次元接続・公開釣合い/仕事・不正参照方向拒否/有限fallback・保存失効・job診断のVerification、workspace成功。Validation・最終headゲート未確認） |
| 7c | フランジ位置の標準形式ゲート | sepika-core / sepika-element / sepika-design-jp / sepika-job / sepika-solver / sepika-app / sepika-mcp | panel_zone.rs / joint_wiring / prepare.rs / viewer / design_check.rs / summary.rs | [Issue505独立期待値・状態](フランジ位置の標準形式ゲート_Issue505_2026-10.md) | 横断 | 🔶（#455実入力・正式段違い式・公開全構成CI・Validation未） |
| 7d | 仕口パネルの参照柱 | sepika-core / sepika-element / sepika-design-jp / sepika-job / sepika-app | panel_zone.rs / springs/panel.rs / springs/panel_gen.rs / joint_wiring / prepare.rs / result_contract_tests.rs | [Issue585受入と確認状態](仕口パネル参照柱_Issue585_2026-10.md) | 横断 | 🔶（直接・関連回帰Verification成功、原典/手動GUI/Validation・最終headゲート未） |
| 7e | 明示仕口パネルの採用G入力検証 | sepika-core / sepika-element / sepika-solver / sepika-job / sepika-app | panel_zone.rs / springs/panel.rs / tests/panel_zone.rs / prepare.rs / result_contract_tests.rs | [Issue728の公開入口と正常対照](明示仕口パネル採用G_Issue728_2026-10.md) | 横断 | 🔶（限定Verification、後続ゲート・Validation未） |
| 7h | 仕口パネルの高軸力降伏域 | sepika-element / sepika-solver / sepika-app / sepika-mcp | springs/panel.rs / nonlinear / result_contract_tests.rs | [Issue753の独立期待値と実応答](仕口パネル高軸力降伏域_Issue753_2026-10.md) | 横断 | 🔶（適用域・履歴・途中solver試行の拒否・旧35失効のVerification。Validation・最終ゲート未） |
| 7g | 仕口パネルの直接revert | sepika-element | springs/panel.rs | [Issue751の復元・再試行契約](仕口パネルrevert復元_Issue751_2026-10.md#直接verification) | 要素 | 🔶（直接Verification成功、実験Validation・最終ゲート未確認） |
| 7f | 仕口パネルの軸力更新と非線形履歴 | sepika-element / sepika-material | springs/panel.rs / uniaxial/bilinear.rs | [Issue724の状態対応と独立照合](仕口パネル軸力と履歴_Issue724_2026-10.md) | 要素 | 🔶（独立Verification4成功。直接revertは#7gで修正・検証、#752・原典§6・Validation未確認、高nは#7h） |
| 8 | 線形静的解析 | sepika-solver | linear.rs | `test_*`（座標変換回帰 `test_beam_to_global_transverse_uses_correct_inertia` 含む） | P2 | ✅ |
| 9 | 固有値解析 | sepika-solver | eigen.rs | `test_1dof_period` | P2 | ✅ |
| 10 | 地震力標準式・適用診断 | sepika-load / sepika-solver / sepika-app / sepika-mcp | ai.rs / statics/analysis/seismic.rs / app/tests.rs / design/holding.rs / nonlinear/pushover/driver.rs / server.rs | [Issue486の独立期待値・入口対応表](地震力標準式と適用診断_Issue486_2026-10.md) | P2 | 🔶（標準式Verification、精算/PH/地域適用等は未対応） |
| 10b | 参照鋼の設計自重密度防護 | sepika-core / sepika-load / sepika-job / sepika-app / sepika-mcp | material.rs、self_weight_calc.rs、beam_design.rs、wall_plate_load.rs、wall_attached.rs、prepare.rs | [Issue501の独立期待値・実入口](参照鋼の高密度防護_Issue501_2026-10.md#独立期待値と直接試験) | 横断 | 🔶（同コード公開全構成成功、最終headゲート後続、実験未） |
| 10d | 床板の同期物理質量 | sepika-core / sepika-load / sepika-job / sepika-app | slab.rs、slab_load.rs、story_gen/generate.rs | [Issue583の独立期待値・実入口](床板の同期物理質量_Issue583_2026-10.md#対象と独立期待値) | 横断 | 🔶（全CI・freshレビュー後続、実験未） |
| 10c | 鋼床板の設計自重密度防護 | sepika-core / sepika-load / sepika-job / sepika-app / sepika-mcp | slab.rs、floor/mod.rs、auto_loads.rs、story_gen/generate.rs、app/actions/design/check.rs | [Issue581の独立期待値・実入口](鋼床板の高密度防護_Issue581_2026-10.md#独立期待値と直接試験) | 横断 | 🔶（階physicalは別課題#583、最終headゲート未完、実験未） |
| 10a | 地震重量鮮度・共通準備 | sepika-core / sepika-load / sepika-job / sepika-app / sepika-mcp | weight_preparation.rs / prepare.rs / actions/loads.rs / period.rs / server.rs | [Issue439の実入口試験表](地震重量の鮮度と共通準備_Issue439_2026-10.md#独立期待値と直接試験) | 横断 | 🔶（入口Verification、法適用・実測は別） |
| 11 | 床荷重分割 | sepika-load / sepika-solver | floor/{mod,polygon}.rs / statics/analysis/tests.rs | `polygon::verification::*` / `distribution_verification::*` / `floor::tests::*` / `test_model_issues_warns_floating_plate`（正当な水平全周支持床の警告と解析前チェック）。幾何Verification済み（台形は[Issue438検証](凸台形の独立幾何と格子差_Issue438_2026-10.md)、矩形分類・`floor::tests::issue561_*`・`auto_loads::tests::issue561_*`・App/OVIKAは[Issue561検証](矩形床の直交分類_Issue561_2026-10.md)。Windows境界入力・`issue645_boundary_fixture_preserves_load_resultant_and_xy_first_moments`は同記録の[Issue645追補](矩形床の直交分類_Issue561_2026-10.md#windows境界fixtureの追補issue6452026-10-15)）、実験Validation未実施 | P2 | 🔶 |
| 11a | 壁・間柱の自重支持先 | sepika-load / sepika-app | wall_plate_load.rs / cascade.rs | `wall_plate_load::tests`・`vertical_post_uses_explicit_end_shares`・`明示負担率で密度直接集計とdl集計の階重量が一致する` | [検証記録](壁版の支持範囲判定_2026-09.md) | 🔶（実測照合は未実施） |
| 11b | 主架構支持の解析表現不変性 | sepika-core / sepika-load / sepika-job / sepika-edit / sepika-io / sepika-app | element.rs / region_gen / assignment_region.rs / model/secondary.rs / load/secondary.rs | [Issue597の独立期待値と実入口](主架構支持の解析表現不変性_Issue597_2026-10.md#実入口) | 横断 | 🔶（直接Verification、freshレビュー・最終head全CI後続） |
| 11c | 取付き版と小梁荷重の支持判定 | sepika-core / sepika-load / sepika-job / sepika-app | region_rebuild.rs / wall_region_rebuild.rs / floor/beam_design.rs | [Issue632の独立入力と観測契約](取付き版と小梁荷重の支持判定_Issue632_2026-10.md#独立入力と観測契約) | 横断 | 🔶（変更後ローカル回帰・Clippy成功。文書／最終ゲート未完了） |
| 12 | 荷重組合せ・検定対象 | sepika-core / sepika-job / sepika-app / sepika-mcp | load_combo.rs、design_q0.rs、app/tests.rs、tests.rs | [荷重状態の検証対応](荷重状態と検定対象_Issue487_2026-10.md#直接検証の対応) | P2 | 🔶（原文再取得・Validation未） |
| 13 | 許容応力度設計 | sepika-design-jp | allowable_stress.rs | `test_beam_check_bending_rect_section_hand_calc` 他 | P3 | ✅ |
| 13a | 大梁必要横補剛箇所数（参考） | sepika-design-jp / sepika-app / sepika-mcp | steel/lateral_bracing.rs・member_design_check.rs・app/tests.rs・tests.rs | [Issue488直接受入](大梁横補剛参考数_Issue488_2026-10.md#直接試験) | 横断 | 🔶（参考式Verification、実配置・性能・Validation未） |
| 13c | SM520告示Fと範囲外拒否 | sepika-core / design-jp / element / solver / app / mcp | sm520境界・現役caller実入口・result_contract_tests | [Issue629の固定期待値と実入口](SM520告示基準強度と範囲外拒否_Issue629_2026-10.md#独立期待値と実入口) | 横断 | 🔶（直接Verification、Validation未実施。最終ゲートは[PR #646](https://github.com/hrntsm/SEPIKA/pull/646)） |
| 766 | CFT鋼管材料のGUI編集 | sepika-app / sepika-edit | tables/cft_material_tests.rs、app/cft_material_tests.rs、section_material/element_assign.rs | [Issue766の直接入口・独立質量差・保存と再計算](CFT鋼管材料GUI_Issue766_2026-10.md#直接入口と独立期待値) | GUI／編集 | 🔶（入力Verification、手動GUI・Validation・最終ゲート未実施） |
| 13e | 鋼材F選択と入力停止 | sepika-core / sepika-design-jp / sepika-app / sepika-mcp | material_grade.rs・material_strength/steel.rs・floor.rs・joint_wiring/steel_panel.rs・actions/design/check.rs | [Issue601の独立期待値と実入口](鋼材F選択と入力停止_Issue601_2026-10.md) | 横断 | 🔶（材料/パネルVerification成功、App全479成功・直接/保存4成功、MCP feature144成功、GUI971成功は形状不足追補前、追補後design675/App4/MCP1成功、Validation未実施） |
| 13b | 標準鋼材名の厳密解決 | sepika-core / sepika-io | material_grade.rs・standard_material.rs・stbridge/strength_tests.rs | [Issue590の独立表・公開STB入口](標準鋼材名の厳密解決_Issue590_2026-10.md#独立期待値と実入口) | 横断 | 🔶（名称Verification、種類別F全照合・Validation未） |
| 13d | 用途別せん断面積と二方向検定 | sepika-core / sepika-element / sepika-design-jp / sepika-app | section_shape・steel・frame/beam/construct・shear_area_acceptance_tests | [原5Done・独立期待値・run別証拠](用途別せん断面積と二方向検定_Issue417_2026-10.md)、`surface_radius_tests` / `shear_contract_tests` / `constructed_cantilever_*` | 横断 | 🔶（限定Verification、Validation・正式最終ゲート未確認） |
| 14 | 保有耐力 | sepika-design-jp | holding_capacity.rs | `test_*` | P7 | 🔶 |
| 620 | 自立壁n倍法 | sepika-design-jp / sepika-core / sepika-job / sepika-app | eccentricity::tests::misc620、eccentricity_analysis::misc620_tests、lumped_mass::tests::misc620 | [点検と独立期待値](自立壁n倍法_Issue620_確認記録.md#独立期待値と試験) | 二次設計 | 🔶（有限Verification、2025限定照合は#740、採用整合・Validation未） |
| 740 | 自立壁Rs接続の成立範囲 | 独立線形静力学・有理数算術 | 製品テスト追加なし | [連成2例とRs正規化](自立壁Rs接続_Issue740_調査記録.md#独立線形静力学による成立範囲) | 二次設計 | 🔶（2025限定照合済、D→K・層内合算・δg接続と近似精度は未確定） |
| 718 | Fes 算定不能の伝播 | sepika-design-jp / sepika-app / sepika-job | secondary/holding_capacity・stiffness_ratio・eccentricity、summary、actions/design/holding、lumped_mass | [独立期待値と直接入口](Fes算定不能の伝播_Issue718_2026-10.md#独立期待値と直接入口) | 横断 | 🔶（Scope 内の自動 Verification 成功、手動 GUI・原典/Validation・最終ゲートは未確認） |
| 15 | プッシュオーバー | sepika-solver / sepika-job / sepika-app / sepika-mcp | nonlinear/pushover/driver.rs・tests.rs / compute.rs | [Issue433既存増分回帰](弧長法削除と既存増分回帰_Issue433_2026-10.md#既存回帰と観測) | P5 | 🔶（弧長削除Verification、4方向等は未検証） |
| 15a | 壁単体の確定step応答 | sepika-solver / sepika-app / sepika-job / sepika-mcp | nonlinear/pushover/wall_response.rs・tests.rs, tests/wall_model.rs, ultimate_demand.rs, job/pushover.rs | [Issue443検証](壁の確定ステップ応答_Issue443_2026-10.md)の試験対応表 | — | ✅（Scope内Verification） |
| 15b | 目的別確定層力 | sepika-solver / sepika-app / sepika-mcp | story_response.rs・result_validity.rs・job/pushover.rs | [Issue442検証](目的別確定層力_Issue442_2026-10.md) | — | ✅（Scope内Verification） |
| 15c | 増分部材履歴の有限記録 | sepika-solver / sepika-app | nonlinear/pushover/member_response.rs・step_record.rs・tests.rs / result_contract_tests.rs | [Issue732の公開再現と境界](増分部材履歴の有限記録_Issue732_2026-10.md) | 横断 | 🔶（境界・公開Err・正常履歴と既存記録保持・解析32失効/33保持のVerification。最終ゲート・Validation未） |
| 16 | 壁（TVLEM） | sepika-element | — | — | P5.5 | 対象外 |
| 17 | 時刻歴 | sepika-solver | timehistory.rs | — | P6 | ❌ |
| 17b | 時刻歴の入力・応答時間軸 | sepika-solver / sepika-job / sepika-app / sepika-mcp / sepika-io | dynamic/timehistory・lumped_mass、sample_wave.rs、checkpoint.rs | [Issue610の独立fixtureと実入口](時刻歴時間軸_Issue610_2026-10.md#修正後の実入口と観測) | 横断 | 🔶（焦点Verification成功、手動GUI／Validation未） |
| 17a | GUI 時刻歴の実入力と結果所属 | sepika-core / sepika-app | model/vibration.rs・app/actions/time_history.rs・lumped_mass.rs・result_validity.rs | [Issue611受入表](時刻歴の実入力と結果所属_Issue611_2026-10.md#受入対応表) | 横断 | 🔶（修正後GUI14＋既存617の入口Verification、広域再検査・native操作・Validation未） |
| 18 | 一軸履歴則（Concrete/Bilinear/MP） | sepika-material | uniaxial.rs | `test_concrete_*`/`test_bilinear_*`/`test_menegotto_pinto_*` | P4 | ✅ |
| 19 | 部材履歴則（武田・原点指向・標準・逆行・最大点指向） | sepika-material | hysteresis.rs | `tests/hysteresis_snapshots.rs`/`tests/uniaxial_snapshots.rs`、`zero_increment_*`と実factory/壁/質点系solver（[Issue596検証](ゼロ増分履歴保持_Issue596_2026-10.md)） | P4 | 🔶（ゼロ増分Verification済み、全履歴Validation未） |
| 20 | ファイバ断面（M–φ 積分） | sepika-section / sepika-core / sepika-element | fiber.rs, mn_surface/fibers.rs, section_shape/rounded.rs | `test_section_*`、`rounded_tests`、`rounded_beam_fiber_*`。フィレット・角Rの領域、公開耐力、両質量モードの証拠は [Issue #413 検証](フィレット_角R_断面整合_Issue413_2026-10.md) | P4 | ✅（指定形状の細分化照合。通常配置の一般精度保証ではない） |
| 20a | 微小相似Fiber初期断面 | sepika-math / sepika-element / sepika-solver | dense_lu.rs, frame/fiber/constitutive_tests.rs, panel_zone.rs | `near_diagonal_coupling_roundoff_*`・`similar_*`・`panel_753_public_nonlinear_time_history_*`（[Issue #783検証](微小相似Fiber初期断面_Issue783_2026-10.md)） | P4 | 🔶（数値Verification。実験Validation未） |
| 20b | 物理曲率・非対称断面・剛域付き軸履歴 | sepika-element | frame/fiber/physical_curvature_tests.rs、distributed_curvature_tests.rs、rigid_axial_tests.rs | [保存成果の補完と端部解放の収束検証](保存成果の物理曲率と剛域軸履歴_Issue496_2026-10.md) | P4 | 🔶（有限Verification。実験Validation未） |
| 20c | 保存/失敗時の全要素復元と全固定時刻歴 | sepika-element / sepika-io / sepika-solver | springs/isolator/tests.rs、checkpoint/preserved_tests.rs、common/elem_loop/preserved_tests.rs、timehistory/tests/loaded_recording.rs、tests/general_member_loads.rs | [保存ブランチの残りの復元検証](保存ブランチ496の変更採否_2026-10.md) | 横断 | 🔶（有限Verification。実験Validation未） |
| 21 | 参考骨格算定（M–φ→M–θ、廃止済み） | — | — | 現行の試験対象なし。上記#436の履歴記録を参照 | P4 / #436 / #849 | 廃止済み（単位・拒否契約の検証履歴を保持） |
| 22 | MCP サーバ（rmcp） | sepika-mcp | server.rs, job/*.rs | `model_query`/`model_edit`（壁版・床板・床領域）/`quantity_takeoff`/`analysis_run`/`result_get`/`analysis_status`（`--features mcp` で CI 検証。`tests.rs` + `server.rs` 統合テスト）。**未公開**: `model.load`/`model.save`/`report.export` | P8 | 🔶 |
| 23 | ST-Bridge 入出力 | sepika-io | stbridge/mod.rs、tests/stbridge_brace_content.rs | [Issue625直接入口と正常対照](未割当線材のSTB出力拒否_Issue625_2026-10.md#独立期待値と直接入口)、 [Issue647実描画・既存入口](STB書出の開発中表示_Issue647_2026-10.md#直接検証と観測)、 `test_roundtrip_*`、`source_story`（[原階所属](原階所属_Issue497_2026-10.md)）、[Issue550全文XSD・意味照合](鋼ブレースXML文字子_Issue550_2026-10.md#独立fixtureと公開入口) | P8 | 🔶（鋼ブレース文字子の限定Verificationは✅） |
| 23d | STB組立BOXの入力境界 | sepika-io / sepika-app | tests/stbridge_built_box.rs、app/reference_identity_acceptance_tests.rs | [Issue683の読込結合](STB異厚組立BOX_Issue683_確認記録.md#要素テスト索引) | 横断 | 🔶（異厚の公開入口・参照/属性報告・質量/保存と不正寸法のVerification、Validation・手動GUIは未確認） |
| 23c | STB CFT鋼管の標準密度 | sepika-io / sepika-core | stbridge/import/assemble.rs、tests/stbridge_cft_steel_density.rs | [Issue741の公開入口と独立面積期待値](STB_CFT鋼管標準密度_Issue741_2026-10.md#独立期待値と公開入口) | 横断 | 🔶（公開入口・独立面積質量のVerification成功、Validation・最終ゲート未確認） |
| 23a | STB標準入力限定 | sepika-io / sepika-app | stbridge/import、tests/stbridge_standard_input.rs、tests/stbridge_standard_rebar.rs、reference_identity_acceptance_tests.rs | [Issue660の独立入力と観測契約](STB標準入力限定_Issue660_2026-10.md#独立入力と観測契約) | 横断 | 🔶（両入口直接10・IO200・App lib837・文書検査成功。追加feature回帰1失敗後、App試験移行対象6成功。全体再実行・親最終監査・最終ゲート未確認） |
| 23b | STB断面系列別ID | sepika-io / sepika-app | stbridge/import、tests/stbridge_namespace.rs、tests/stbridge_section_series.rs、tests/stbridge_section_reference_boundaries.rs、reference_identity_acceptance_tests.rs | [Issue571の独立入力と予定観測](STB断面系列別ID_Issue571_2026-10.md#独立入力と予定観測) | 横断 | 🔶（series12・boundary6・namespace9・App自動9・IO227・最終feature1322成功。文書修正後の再検査/レビュー・最終ゲート未確認） |
| 24 | 編集トランザクション（EditCommand/Undo） | sepika-edit | lib.rs | `test_*`、`source_story_*`（[原階所属](原階所属_Issue497_2026-10.md)。MCP `model_edit` は原階所属・壁版・床板・床領域を配線。他コマンドは未 → [未検証一覧 §6](未検証一覧.md)） | P3/P8 | ✅ |
| 24a | 割当領域の同一性・孤立版診断 | sepika-core / sepika-edit / sepika-job / sepika-app / sepika-mcp | model/assignment_region.rs、secondary.rs、node_member.rs、prepare.rs | [Issue506検証](割当領域同一性_Issue506_2026-10.md)の独立fixtureと実入口 | #506 | ✅（状態契約Verification） |
| 25 | 終局検定（塑性 Qsu・付着 Qbu・軸 Nuc/Nut・2軸せん断・接合部 Vju/Qdu・CFT 軸終局+N-M） | sepika-design-jp | ultimate/{rc_shear,rc_axial,joint,cft,cft_nm,mod}.rs | `test_rc_shear_qsu_plastic_*`/`test_rc_joint_ultimate_*`/`test_cft_*`/`test_cft_short_column_mu_*`/`test_biaxial_*`/`test_collect_*_ultimate_checks_*`、Case2の証拠区分は[§6-c](終局検定_参照実装照合.md#6-c-cft-中柱長柱の-n-m-相互作用) | P7 | 🔶 |
| 26 | 数量積算（部位別のコンクリート・型枠・鉄筋・鉄骨・継手個所） | sepika-design-jp | quantity/{mod,member,rebar}.rs | `quantity::member::tests::*`（手計算照合）/`quantity::tests::*`（走査・分類）/`summary::tests::test_quantity_csv_from_sample_model`（CSV 一気通貫）/`test_quantity_takeoff_json_column`（MCP） | 横断 | 🔶 |
| 26c | 非節点二次部材の数量 | sepika-design-jp / sepika-app / sepika-mcp / sepika-edit / sepika-io | quantity・quantity_view・summary・query・secondary・ovika | [Issue557の元6受入と実入口](非節点二次部材数量_Issue557_2026-10.md#元の受入6項目と実入口) | 横断 | 🔶（自動実入口・広域Verification成功、nativeGUI/transport/Validation未） |
| 26d | 小梁偏心別の数量集約 | sepika-design-jp | quantity/{mod,tests}.rs | [Issue792の独立期待値と直接試験](小梁偏心別の数量集約_Issue792_2026-10.md#独立期待値と直接試験) | 数量 | 🔶（集約条件Verification、施工Validation未） |
| 26e | 二次部材の共通実材判定 | sepika-core / sepika-load / sepika-design-jp / sepika-app / sepika-solver | model/secondary.rs・cascade・floor・tables/secondary.rs・precheck.rs | [Issue794の独立期待値と製品経路](二次部材の共通実材判定_Issue794_2026-10.md#独立期待値と製品経路) | 横断 | 🔶（判定・利用経路Verification、手動GUI・Validation未） |
| 26b | RC壁方向別数量・共有設定 | sepika-core / sepika-edit / sepika-design-jp / sepika-app / sepika-io / sepika-mcp | quantity・quantity_view・summary・query・model・ovika | [Issue536の独立期待値と実入口](RC壁方向別数量_Issue536_2026-10.md#実入口試験への対応) | 横断 | 🔶（Scope内Verification成功、Validation未実施） |
| 26b | 主架構線材の2節点入力 | sepika-core / sepika-edit / sepika-io / sepika-job / sepika-solver / sepika-design-jp / sepika-app / sepika-mcp | model/element・aggregate、refs、ovika、prepare、precheck・linear/mod | [Issue562のkind×節点数×入口matrix](主架構線材の2節点入力_Issue562_2026-10.md#独立期待値と入口matrix) | 横断 | 🔶（Windows入口・自動App/egui・MCP Verification成功、手動GUI未実施、最終headゲート後続） |
| 26a | 梁型枠の床接触控除 | sepika-design-jp / sepika-app / sepika-mcp | quantity/contact.rs・tests.rs、tests/beam_contact_quantity.rs | [Issue451検証](梁型枠接触和集合_Issue451_2026-10.md)の独立期待値・診断・実入口対応表 | 横断 | 🔶（限定Verification、一般形状・実施工未検証） |
| 27b | 鋼材名の自動強度割増認識 | sepika-core / sepika-element / sepika-solver / sepika-app | material_grade.rs・factory/hinge_view.rs・pushover/tests.rs・result_contract_tests.rs | [Issue599の独立閾値・実入口](鋼材名の厳密な強度割増認識_Issue599_2026-10.md#独立期待値と実入口) | 横断 | 🔶（直接Verification、freshレビュー・最終CIは後続） |
| 27a | ST-Bridge標準強度と省略 | sepika-core / sepika-io / sepika-edit / sepika-app / sepika-mcp | model/strength.rs、stbridge/strength_tests.rs、app/tests.rs、tests.rs | [Issue520の直接回帰表](標準強度_Issue520_2026-10.md#独立期待値と入口) | 横断 | ✅（Scope内Verification） |
| 27 | 材料グレード対応表（F 値・鉄筋・Fc・プリセット） | sepika-core | material_grade.rs | `material_grade::tests::*`（告示値一致） | 横断 | ✅ |
| 28 | 二次部材小梁の分配 Span 検定 | sepika-load / sepika-app | floor/joist_design.rs, check.rs | `distribution_loads_on_shared_joist_match_average_width` / `split_slab_edges_compose_onto_full_joist` / `span_attaches_to_nearest_joist_only` / `perimeter_parallel_joist_does_not_steal_beam_span` / `joist_distribution_cover_rejects_half_span` / `shared_joist_expects_both_slabs` / `missing_expected_slab_is_not_ready` / `zero_expected_axis_does_not_receive_spans` / `joist_design_checks_cover_imported_secondary_members` | 横断 | ☑ |
| 29 | 耐震スリット（辺ごとの縁切り。袖壁・腰壁垂壁・剛域・耐震壁判定・自重） | sepika-element / sepika-load | wall/misc_wall.rs, frame/beam/{construct,rigid_zone}.rs, story_gen/self_weight_calc.rs | `test_column_face_slit_drops_wing_wall_but_keeps_girder_strip` / `test_strip_height_follows_beam_face_slit` / `test_any_slit_breaks_seismic_wall` / `柱際スリットのある側は剛域の袖壁張り出しを持たない` / `test_column_face_slit_wall_is_collected_as_misc_wall` / `test_column_face_slit_is_per_side` / `test_column_face_slit_does_not_change_self_weight_destination` / `test_bottom_beam_face_slit_sends_self_weight_to_top` / `test_top_beam_face_slit_sends_self_weight_to_bottom`（柱際は行き先不変、梁際の切れ側で全量移動） | 横断 | 🔶 |
| 30 | 壁版の要素生成判定と可視化 | sepika-core / sepika-load / sepika-app | model/wall_plate.rs, wall_expand.rs, viewer/scene.rs | `test_becomes_element_agrees_with_generated_elements` / `test_becomes_element_requires_section` / `test_boundary_coords_with_uses_supplied_coords` / `囲まれた壁版は境界節点が全て構面上にあるときだけ描く` / `取り付く壁版は取付き先の節点で構面を判定する` | 横断 | ☑ |
| 30a | RC壁線形軸・面内曲げの調査 | sepika-element / sepika-solver | wall/wall_element.rs・factory/mod.rs・tests/wall_system_linear.rs | [Issue652の実入口と独立条件](RC壁線形軸曲げ剛性_Issue652_調査記録.md#独立期待値と実入口) | 要素／線形 | 🔶（指定原典の役割限定照合。現行7試験成功、総断面候補未採用・未検証） |
| 30b | RC開口壁の線形モデル監査 | sepika-element / sepika-solver | tests/wall_system_linear.rs | [Issue821の48条件監査](RC開口壁_Issue821_モデル監査.md#実入口の再現) | 要素／線形 | 🔶（現行分岐のみ確認、物理Validation・採否未了） |
| 31 | Fc>60 の NewRC 外挿 | sepika-material / sepika-element | newrc.rs, uniaxial/concrete_cyclic.rs, frame/fiber/mod.rs | Fc≤60 の材料 E 反映を維持し、Fc>60 の NewRC 固有 Ec 外挿、構成則選択・圧縮包絡線・ファイバー生成・3履歴則の応答をコードテストで照合済み。実験・原典適用範囲外の妥当性確認は未実施（詳細は [NewRC高強度コンクリート外挿_2026-09.md](NewRC高強度コンクリート外挿_2026-09.md)） | 横断 | 🔶 |
| 32a | OVIKA保存成果の計算契約 | sepika-core / sepika-app | result_contract.rs・result_validity.rs・actions/design・result_contract_tests.rs | [Issue587の実OVIKA・実入口](保存成果の計算契約版_Issue587_2026-10.md#独立期待値と実入口) | 横断 | 🔶（直接Verification、freshレビュー・最終全構成は後続） |
| 32b | MCP保存成果の生成元と公開 | sepika-core / sepika-io / sepika-mcp | results/fs_store.rs・persist.rs・server_lifecycle_tests.rs | [Issue589の独立期待状態と実入口](MCP保存成果の生成元_Issue589_2026-10.md#独立期待状態と実入口) | 横断 | 🔶（直接Verification。最終ゲートは[PR #651](https://github.com/hrntsm/SEPIKA/pull/651)） |
| 32b | 静的検定の生成入力 | sepika-core / sepika-app | result_validity.rs・actions/design/check.rs・design_input_tests.rs | [Issue602の独立数値・実入口](静的検定の生成入力_Issue602_2026-10.md#実入口と観測) | 横断 | 🔶（直接Verification、freshレビュー・最終全CIは後続） |
| 32 | OVIKA の named MessagePack 永続化 | sepika-io / sepika-app | ovika.rs, tests/full_model.rs | `saved_model_fields_are_named_and_order_independent` / `ovika_roundtrip_preserves_model_and_results` 他。default・破損・復元モデル検証とモデル単体のサイズ・codec 比較は [検証記録](OVIKA_named_MessagePack_2026-10.md) を参照 | 横断 | ✅ |
| 33 | 単一リップ溝形材の未検定 | sepika-design-jp / sepika-app | steel/lip_channel.rs、tests/lip_channel.rs、viewer/check_ratio.rs | `steel::lip_channel::tests::*` / `lip_channel_*`。詳細は[未検定](リップ溝形材_未検定_Issue509_2026-10.md)と[幾何成立](リップ溝形の幾何成立条件_Issue544_2026-10.md)の記録 | 横断 | 🔶（未検定契約・幾何入力Verification。耐力Validation未） |
| 34 | RC鉄筋比の単位・分母 | sepika-core / sepika-element / sepika-design-jp / sepika-solver | rc_capacity.rs、factory/{springs,input_check}.rs、rc/beam_nonlinear.rs、pushover/shear_yield.rs | [Issue #435の直接回帰表](RC鉄筋比契約_Issue435_2026-10.md#独立fixtureと直接回帰) | 横断 | 🔶（契約Verification、T形解析・原著照合・Validationは残る） |
| 35 | 耐震壁の未検定状態 | sepika-design-jp / sepika-app / sepika-mcp | wall_check.rs、joint_wiring/tests.rs、tests/wall_check_status.rs、viewer/check_ratio.rs、job/design_check.rs | `wall_status_*` / `wall_tooltip_real_egui_*` / `wall_ng_real_egui_*` / `wall_output_assembly_*`。[Issue495検証](耐震壁_未検定_Issue495_2026-10.md) | 横断 | 🔶（状態契約のみ） |
| 38 | 外部ID入力拒否 | sepika-mcp / sepika-io | edit、server、stbridge/import・export | [Issue603の直接入口](外部ID入力拒否_Issue603_2026-10.md#直接入口と観測) | 横断 | 🔶（入口Verification、最終headゲートは後続） |
| 37 | 二次CFTの全入口拒否 | sepika-core / sepika-edit / sepika-io / sepika-app / sepika-design-jp / sepika-solver / sepika-mcp | model/aggregate・section、ovika、quantity、actions/io、precheck | [Issue539の直接入口と正常対照](二次CFT入口拒否_Issue539_2026-10.md#直接入口と観測) | 横断 | 🔶（入口Verification、最終headゲートは後続） |
| 36a | 直接線形求解 | sepika-math / sepika-solver / sepika-core / sepika-app | cholesky・lu・solver・statics/analysis/linear・result_contract_tests | [Issue598独立期待値と実入口](直接線形求解_Issue598_2026-10.md#独立期待値と実入口) | 横断 | 🔶（Verification、実験Validation・最終headゲート未） |
| 36 | 静的設定の公開経路一致 | sepika-solver / sepika-element / sepika-job / sepika-app / sepika-mcp | statics/linear、statics/analysis、factory、compute、actions/linear_static、tests | [Issue595の直接入口と回帰](静的設定採用_Issue595_2026-10.md#直接入口と回帰) | 横断 | 🔶（Verification、Validation・最終headゲート未） |
| 600 | RC梁Q0の符号付き端反力合成 | sepika-job / sepika-design-jp / sepika-app / sepika-mcp | design_q0.rs・rc/beam.rs・th_detail.rs | [Issue600の受入対応](RC梁Q0の符号付き端反力合成_Issue600_2026-10.md#接続保存受入の対応) | 横断 | 🔶（直接/追加受入・保存・関連構成Verification成功、手動GUI/Validation・最終ゲート未確認） |
| 633 | 符号付き単純梁反力 | sepika-load / sepika-element / sepika-app | floor/fem.rs・story_gen/reactions.rs・frame/member_load.rs・viewer/cmq.rs | [Issue633の独立値・公開入口](符号付き単純梁反力_Issue633_2026-10.md#独立verification) | 横断 | 🔶（直接Verification、原本／Validation・最終CIは後続） |
| 615 | 節点TSVの一括拒否と状態保持 | sepika-app | app/node_grid.rs・grid/core.rs・grid/widget.rs・tables/nodes.rs | [実入口と期待値](節点TSV一括拒否_Issue615_2026-10.md#独立期待値と実入口) | 自動egui／編集 | 🔶（SEPIKA側Verification、外部Excel/OS手操作未実施） |
| 546 | 節点・材料直接入力の有限判定 | sepika-app / sepika-edit | tables/nodes.rs・tables/materials.rs・section_material/material.rs | [直接入力matrix](節点材料の直接有限入力_Issue546_2026-10.md#独立期待値と実入口) | GUI／編集 | 🔶（直接Verification、最終head CI後続） |
| 475 | 未変形座標による編集入力 | sepika-app | app/coordinate_edit_tests.rs・app/panels/draw_tools.rs・load_editor.rs | [独立期待値と実入口](未変形編集入力_Issue475_2026-10.md#独立期待値と実入口) | GUI／編集 | 🔶（自動Verification成功、手動GUI未実施。最終ゲートは[PR #661](https://github.com/hrntsm/SEPIKA/pull/661)） |
| 500 | CMQ の M 成分の塗り分割 | sepika-app | viewer/cmq.rs・viewer/diagram.rs | [独立期待値と実入口](CMQのM成分塗り分割_Issue500_2026-10.md#独立期待値と実入口) | GUI／表示 | 🔶（Scope内Verification成功、手動GUI未実施。最終ゲートは[PR #662](https://github.com/hrntsm/SEPIKA/pull/662)） |
| 635 | 固定距離部材荷重の支間内検証 | sepika-core / sepika-edit / sepika-solver / sepika-io / sepika-app / sepika-mcp / sepika-job | model/load.rs・node_member.rs・story_copy.rs・precheck.rs | [Issue635の独立受入と実入口](固定距離部材荷重の支間内検証_Issue635_2026-10.md#独立期待値と実入口) | 横断 | 🔶（入力境界Verification、最終headゲートは後続） |

凡例: ✅ 記載範囲は完了・🔶 未実装または未検証の範囲が残る・❌ 未実装・対象外（採用しない）。
壁横筋βs・参考骨格の単体/GUI共通/MCP実ジョブ検証は[Issue503検証](壁横筋入力と終局割線骨格_Issue503_2026-10.md)を参照（Verification ✅、実験Validation 🔶）。
無開口Qc・面積の是正後の独立算術／直接一括・GUI・公開MCP入口と編集保存失効は[Issue534検証](RC壁ひび割れ耐力_Issue534_2026-10.md)を参照（Scope内Verification成功、実験Validation未実施。最終ゲートは[PR #658](https://github.com/hrntsm/SEPIKA/pull/658)）。Issue503の旧Qc値から今回の成功を推定しない。

#16 壁（TVLEM）は採用しない（[ADR 0013](../adr/0013-adopt-wall-element-model.md)）。耐震壁は壁エレメント置換モデルとして検証する（下表 #29・#30 と[未検証一覧 §3](未検証一覧.md)）。

終局需要の用途・符号・完全性（索引#25の横断入口）は[Issue565記録](終局需要の重力契約_Issue565_2026-10.md)を参照する。App/collector/jobの実測、途中失敗と修正後の成功、独立期待値と耐力式Validation未確認を区別する（🔶）。

各 # の修正履歴・監査結果の詳細は [§レポート目録](#レポート目録) の該当レポートを参照。
未完了の要約は [未検証一覧.md](未検証一覧.md) を参照。

[Fes 算定不能の伝播（Issue718）](Fes算定不能の伝播_Issue718_2026-10.md) は、通常式の独立期待値・実集計と算定不能の自動 GUI・CSV・実保存/再読込み・再計算の Verification を記録する（🔶、手動 GUI・原典/Validation・最終ゲートは未確認）。

## 1 次参照: 手計算／理論解

本ソフトの V&V は、原則として **手計算／理論解** を一次基準とする。手計算・理論解・告示式の自己整合で確認する範囲と、規準本体・実験による確認が必要な範囲を分ける。

**実測／商用ソフト照合は補助**（入手できれば追加）であり、なくてもビルド・単体テスト・一次 V&V は通る。

壁谷澤 TVLEM は採用しないため（[ADR 0013](../adr/0013-adopt-wall-element-model.md)）、例外なく手計算／理論解で判定する。耐震壁は壁エレメント置換モデルとして検証する（残項目は[未検証一覧 §3](未検証一覧.md)）。

## パネルゾーンの検証限界

パネルゾーンの実装・テスト対応は本書の要素→テスト索引を参照する。
独立した入力条件と導出を確認できない数値例は、参照解として扱わない。
規準本体の照合と実験Validationは別途必要である。

## 決定性テスト

全解析種別で「CPU・単一スレッドでビット一致」を検証（R28）。並列／Parquet書込は値一致で検証（ビット一致保証外）。

| 解析種別 | 状態 | ファイル |
|----------|------|----------|
| 線形静的 | ✅ | linear.rs |
| 疎行列組立 | ✅ | sparse.rs |
| Cholesky 分解 | ✅ | cholesky.rs |
| 固有値 | ✅ | eigen.rs |
| 時刻歴 | 🔶 | timehistory.rs（時刻歴の実装後に本格化） |
| プッシュオーバー | 🔶 | pushover.rs（増分解析の実装後に本格化） |
| 並列バッチ（値一致） | ✅ | sepika-solver/tests/parallel_batch.rs（並列時のケース並列バッチが個別解と一致） |

## 性能ベンチマーク

`criterion` による性能ベンチマーク（線形静的・固有値・プッシュオーバー1ステップ・時刻歴1ステップ）の計測は CI 導入時に整備予定。

並列計算（ケース並列バッチ・faer 内部並列）の速度比は
`cargo run -p sepika-solver --example parallel_bench --release` で計測できる
（ドキュメントサイト 5.10 並列計算に参考値を記載）。

採用RC梁αyと総角・追加角の直接検証は [Issue #523](RC梁αyと追加端ばね_Issue523_2026-10.md) に記録する。

## SRC材料領域の共通入口

[Issue552](SRC材料領域_Issue552_2026-10.md)の`src552_*`試験は、共通材料生成・実要素・OVIKA編集/保存・MN表示cacheと準備診断を対応づける。限定幾何のVerificationであり、SRC全履歴則のValidationは未実施。

RC壁の方向入力・材料役割の横断入口は[Issue535検証](RC壁方向別入力_Issue535_2026-10.md)のelement/GUI/MCP fixtureを参照する（🔶、入力VerificationとValidationを区別）。

RC壁の同一face内力・初期耐力の横断入口は[Issue617検証](RC壁同一faceと初期耐力_Issue617_2026-10.md)の`wall/static_face.rs`・`nonlinear/wall_reference.rs`・`joint_wiring/tests.rs`・App/MCP fixtureを参照する（🔶、独立算術・人工断面力・実求解・実入口を区別）。

RC壁の二方式とGUI/採用run値/失効の横断入口は[Issue572検証](RC壁せん断耐力方式_Issue572_2026-10.md)を参照する。独立算術と実入口を区別する。

一般部材荷重の横断入口は[Issue716検証](一般部材荷重_Issue716_2026-10.md)の `general_member_loads.rs`・`frame::fiber::member_load_tests`・時刻歴 `rollback.rs` を参照する。
