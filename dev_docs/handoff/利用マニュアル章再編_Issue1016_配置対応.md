# 利用マニュアル章再編の配置対応

作成日: 2026-10-30

[Issue #1016](https://github.com/hrntsm/SEPIKA/issues/1016) の章再編で、移動前の全126 Markdownと内容の正本を対応付けた記録。式・判定の正否は変更しない。各項目の進捗はこの表で管理しない。

## 全ページの対応

「全節維持」は見出し以下の内容を保持することを示す。分割ページの正本は後段の節対応で示す。

| 移動前（docs/からの相対パス） | 配置・正本 | 扱い |
| --- | --- | --- |
| `SUMMARY.md` | [SUMMARY.md](../../docs/SUMMARY.md) | 章入口・案内を更新 |
| `calc_basis/01_荷重/01_地震力.md` | [calc_basis/01_荷重/01_地震力.md](../../docs/calc_basis/01_荷重/01_地震力.md) | 維持・全節維持 |
| `calc_basis/01_荷重/02_積載荷重.md` | [calc_basis/01_荷重/02_積載荷重.md](../../docs/calc_basis/01_荷重/02_積載荷重.md) | 維持・全節維持 |
| `calc_basis/01_荷重/03_床荷重の分配.md` | [calc_basis/01_荷重/03_床荷重の分配.md](../../docs/calc_basis/01_荷重/03_床荷重の分配.md) | 維持・全節維持 |
| `calc_basis/01_荷重/04_荷重組合せ.md` | [calc_basis/01_荷重/04_荷重組合せ.md](../../docs/calc_basis/01_荷重/04_荷重組合せ.md) | 維持・全節維持 |
| `calc_basis/01_荷重/05_地震用重量_層データの生成.md` | [calc_basis/01_荷重/05_地震用重量_層データの生成.md](../../docs/calc_basis/01_荷重/05_地震用重量_層データの生成.md) | 維持・全節維持 |
| `calc_basis/01_荷重/06_自重の長期応力解析への接続.md` | [calc_basis/01_荷重/06_自重の長期応力解析への接続.md](../../docs/calc_basis/01_荷重/06_自重の長期応力解析への接続.md) | 維持・全節維持 |
| `calc_basis/01_荷重/07_荷重の入力.md` | [calc_basis/01_荷重/07_荷重の入力.md](../../docs/calc_basis/01_荷重/07_荷重の入力.md) | 分割（節対応参照） |
| `calc_basis/01_荷重/08_床の断面と自重.md` | [calc_basis/01_荷重/08_床の断面と自重.md](../../docs/calc_basis/01_荷重/08_床の断面と自重.md) | 分割（節対応参照） |
| `calc_basis/01_荷重/09_壁の断面と自重.md` | [calc_basis/01_荷重/09_壁の断面と自重.md](../../docs/calc_basis/01_荷重/09_壁の断面と自重.md) | 分割（節対応参照） |
| `calc_basis/01_荷重/README.md` | [calc_basis/01_荷重/README.md](../../docs/calc_basis/01_荷重/README.md) | 章入口・案内を更新 |
| `calc_basis/02_材料/01_一軸材料構成則.md` | [nonlinear/01_一軸材料構成則.md](../../docs/nonlinear/01_一軸材料構成則.md) | 移動・全節維持 |
| `calc_basis/02_材料/02_部材履歴則.md` | [nonlinear/02_部材履歴則.md](../../docs/nonlinear/02_部材履歴則.md) | 移動・全節維持 |
| `calc_basis/02_材料/03_材料強度.md` | [calc_basis/02_材料/03_材料強度.md](../../docs/calc_basis/02_材料/03_材料強度.md) | 維持・全節維持 |
| `calc_basis/02_材料/04_標準材料一覧.md` | [calc_basis/02_材料/04_標準材料一覧.md](../../docs/calc_basis/02_材料/04_標準材料一覧.md) | 維持・全節維持 |
| `calc_basis/02_材料/05_構造種別の判定.md` | [calc_basis/02_材料/05_構造種別の判定.md](../../docs/calc_basis/02_材料/05_構造種別の判定.md) | 維持・全節維持 |
| `calc_basis/02_材料/README.md` | [calc_basis/02_材料/README.md](../../docs/calc_basis/02_材料/README.md) | 章入口・案内を更新 |
| `calc_basis/03_断面性能/01_主要定数.md` | [calc_basis/03_断面性能/01_主要定数.md](../../docs/calc_basis/03_断面性能/01_主要定数.md) | 維持・全節維持 |
| `calc_basis/03_断面性能/02_断面諸量.md` | [calc_basis/03_断面性能/02_断面諸量.md](../../docs/calc_basis/03_断面性能/02_断面諸量.md) | 維持・全節維持 |
| `calc_basis/03_断面性能/03_せん断有効断面積_As.md` | [calc_basis/03_断面性能/03_せん断有効断面積_As.md](../../docs/calc_basis/03_断面性能/03_せん断有効断面積_As.md) | 維持・全節維持 |
| `calc_basis/03_断面性能/04_コンクリートのヤング係数_Ec.md` | [calc_basis/03_断面性能/04_コンクリートのヤング係数_Ec.md](../../docs/calc_basis/03_断面性能/04_コンクリートのヤング係数_Ec.md) | 維持・全節維持 |
| `calc_basis/03_断面性能/05_耐震壁のせん断形状係数_κ.md` | [calc_basis/03_断面性能/05_耐震壁のせん断形状係数_κ.md](../../docs/calc_basis/03_断面性能/05_耐震壁のせん断形状係数_κ.md) | 維持・全節維持 |
| `calc_basis/03_断面性能/06_SRC_CFT_の等価断面性能.md` | [calc_basis/03_断面性能/06_SRC_CFT_の等価断面性能.md](../../docs/calc_basis/03_断面性能/06_SRC_CFT_の等価断面性能.md) | 維持・全節維持 |
| `calc_basis/03_断面性能/07_ファイバー断面.md` | [nonlinear/03_ファイバー断面.md](../../docs/nonlinear/03_ファイバー断面.md) | 移動・全節維持 |
| `calc_basis/03_断面性能/README.md` | [calc_basis/03_断面性能/README.md](../../docs/calc_basis/03_断面性能/README.md) | 章入口・案内を更新 |
| `calc_basis/04_要素剛性/01_ティモシェンコ梁要素.md` | [calc_basis/04_要素剛性/01_ティモシェンコ梁要素.md](../../docs/calc_basis/04_要素剛性/01_ティモシェンコ梁要素.md) | 維持・全節維持 |
| `calc_basis/04_要素剛性/02_トラス要素.md` | [calc_basis/04_要素剛性/02_トラス要素.md](../../docs/calc_basis/04_要素剛性/02_トラス要素.md) | 維持・全節維持 |
| `calc_basis/04_要素剛性/03_部材荷重.md` | [calc_basis/04_要素剛性/03_部材荷重.md](../../docs/calc_basis/04_要素剛性/03_部材荷重.md) | 維持・全節維持 |
| `calc_basis/04_要素剛性/04_MITC4_シェル要素.md` | [calc_basis/04_要素剛性/04_MITC4_シェル要素.md](../../docs/calc_basis/04_要素剛性/04_MITC4_シェル要素.md) | 維持・全節維持 |
| `calc_basis/04_要素剛性/05_壁エレメントモデル.md` | [calc_basis/04_要素剛性/05_壁エレメントモデル.md](../../docs/calc_basis/04_要素剛性/05_壁エレメントモデル.md) | 分割（節対応参照） |
| `calc_basis/04_要素剛性/06_パネルゾーン.md` | [calc_basis/04_要素剛性/06_パネルゾーン.md](../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md) | 分割（節対応参照） |
| `calc_basis/04_要素剛性/07_ばね要素.md` | [calc_basis/04_要素剛性/07_ばね要素.md](../../docs/calc_basis/04_要素剛性/07_ばね要素.md) | 分割（節対応参照） |
| `calc_basis/04_要素剛性/08_座標変換.md` | [calc_basis/04_要素剛性/08_座標変換.md](../../docs/calc_basis/04_要素剛性/08_座標変換.md) | 維持・全節維持 |
| `calc_basis/04_要素剛性/09_非線形梁要素のモデル化.md` | [nonlinear/04_非線形梁要素のモデル化.md](../../docs/nonlinear/04_非線形梁要素のモデル化.md) | 移動・全節維持 |
| `calc_basis/04_要素剛性/10_RC壁の壁脚内力と初期耐力.md` | [calc_basis/08_部材終局耐力/08_RC壁の壁脚内力と初期耐力.md](../../docs/calc_basis/08_部材終局耐力/08_RC壁の壁脚内力と初期耐力.md) | 移動・全節維持 |
| `calc_basis/04_要素剛性/10_梁要素内部の変位と断面力.md` | [calc_basis/04_要素剛性/10_梁要素内部の変位と断面力.md](../../docs/calc_basis/04_要素剛性/10_梁要素内部の変位と断面力.md) | 維持・全節維持 |
| `calc_basis/04_要素剛性/README.md` | [calc_basis/04_要素剛性/README.md](../../docs/calc_basis/04_要素剛性/README.md) | 章入口・案内を更新 |
| `calc_basis/05_構造解析/01_線形代数ソルバ.md` | [calc_basis/05_構造解析/01_線形代数ソルバ.md](../../docs/calc_basis/05_構造解析/01_線形代数ソルバ.md) | 維持・全節維持 |
| `calc_basis/05_構造解析/02_線形静解析.md` | [calc_basis/05_構造解析/02_線形静解析.md](../../docs/calc_basis/05_構造解析/02_線形静解析.md) | 維持・全節維持 |
| `calc_basis/05_構造解析/03_固有値解析.md` | [calc_basis/05_構造解析/03_固有値解析.md](../../docs/calc_basis/05_構造解析/03_固有値解析.md) | 維持・全節維持 |
| `calc_basis/05_構造解析/04_減衰.md` | [calc_basis/05_構造解析/04_減衰.md](../../docs/calc_basis/05_構造解析/04_減衰.md) | 維持・全節維持 |
| `calc_basis/05_構造解析/05_時刻歴応答解析.md` | [calc_basis/05_構造解析/05_時刻歴応答解析.md](../../docs/calc_basis/05_構造解析/05_時刻歴応答解析.md) | 維持・全節維持 |
| `calc_basis/05_構造解析/07_プッシュオーバー解析.md` | [calc_basis/05_構造解析/07_プッシュオーバー解析.md](../../docs/calc_basis/05_構造解析/07_プッシュオーバー解析.md) | 維持・全節維持 |
| `calc_basis/05_構造解析/08_累積損傷度.md` | [calc_basis/05_構造解析/08_累積損傷度.md](../../docs/calc_basis/05_構造解析/08_累積損傷度.md) | 維持・全節維持 |
| `calc_basis/05_構造解析/09_並列計算.md` | [calc_basis/05_構造解析/09_並列計算.md](../../docs/calc_basis/05_構造解析/09_並列計算.md) | 維持・全節維持 |
| `calc_basis/05_構造解析/10_質点系解析.md` | [calc_basis/05_構造解析/10_質点系解析.md](../../docs/calc_basis/05_構造解析/10_質点系解析.md) | 維持・全節維持 |
| `calc_basis/05_構造解析/README.md` | [calc_basis/05_構造解析/README.md](../../docs/calc_basis/05_構造解析/README.md) | 章入口・案内を更新 |
| `calc_basis/06_一次設計/01_材料の許容応力度.md` | [calc_basis/06_一次設計/01_材料の許容応力度.md](../../docs/calc_basis/06_一次設計/01_材料の許容応力度.md) | 維持・全節維持 |
| `calc_basis/06_一次設計/02_RC_断面検定.md` | [calc_basis/06_一次設計/02_RC_断面検定.md](../../docs/calc_basis/06_一次設計/02_RC_断面検定.md) | 維持・全節維持 |
| `calc_basis/06_一次設計/03_鋼構造_断面検定.md` | [calc_basis/06_一次設計/03_鋼構造_断面検定.md](../../docs/calc_basis/06_一次設計/03_鋼構造_断面検定.md) | 維持・全節維持 |
| `calc_basis/06_一次設計/04_SRC_CFT_断面検定.md` | [calc_basis/06_一次設計/04_SRC_CFT_断面検定.md](../../docs/calc_basis/06_一次設計/04_SRC_CFT_断面検定.md) | 維持・全節維持 |
| `calc_basis/06_一次設計/05_小梁・床の断面検定.md` | [calc_basis/06_一次設計/05_小梁・床の断面検定.md](../../docs/calc_basis/06_一次設計/05_小梁・床の断面検定.md) | 維持・全節維持 |
| `calc_basis/06_一次設計/README.md` | [calc_basis/06_一次設計/README.md](../../docs/calc_basis/06_一次設計/README.md) | 章入口・案内を更新 |
| `calc_basis/07_二次設計/01_保有水平耐力の判定.md` | [calc_basis/07_二次設計/01_保有水平耐力の判定.md](../../docs/calc_basis/07_二次設計/01_保有水平耐力の判定.md) | 維持・全節維持 |
| `calc_basis/07_二次設計/02_構造特性係数_Ds.md` | [calc_basis/07_二次設計/02_構造特性係数_Ds.md](../../docs/calc_basis/07_二次設計/02_構造特性係数_Ds.md) | 維持・全節維持 |
| `calc_basis/07_二次設計/03_部材ランク.md` | [calc_basis/07_二次設計/03_部材ランク.md](../../docs/calc_basis/07_二次設計/03_部材ランク.md) | 維持・全節維持 |
| `calc_basis/07_二次設計/04_形状係数_Fes.md` | [calc_basis/07_二次設計/04_形状係数_Fes.md](../../docs/calc_basis/07_二次設計/04_形状係数_Fes.md) | 維持・全節維持 |
| `calc_basis/07_二次設計/05_層間変形角.md` | [calc_basis/07_二次設計/05_層間変形角.md](../../docs/calc_basis/07_二次設計/05_層間変形角.md) | 維持・全節維持 |
| `calc_basis/07_二次設計/06_主軸の計算.md` | [calc_basis/07_二次設計/06_主軸の計算.md](../../docs/calc_basis/07_二次設計/06_主軸の計算.md) | 維持・全節維持 |
| `calc_basis/07_二次設計/README.md` | [calc_basis/07_二次設計/README.md](../../docs/calc_basis/07_二次設計/README.md) | 章入口・案内を更新 |
| `calc_basis/08_部材終局耐力/01_RC_部材_略算終局耐力.md` | [calc_basis/08_部材終局耐力/01_RC_部材_略算終局耐力.md](../../docs/calc_basis/08_部材終局耐力/01_RC_部材_略算終局耐力.md) | 維持・全節維持 |
| `calc_basis/08_部材終局耐力/02_RC_終局せん断.md` | [calc_basis/08_部材終局耐力/02_RC_終局せん断.md](../../docs/calc_basis/08_部材終局耐力/02_RC_終局せん断.md) | 維持・全節維持 |
| `calc_basis/08_部材終局耐力/03_RC_柱梁接合部_終局耐力.md` | [calc_basis/08_部材終局耐力/03_RC_柱梁接合部_終局耐力.md](../../docs/calc_basis/08_部材終局耐力/03_RC_柱梁接合部_終局耐力.md) | 維持・全節維持 |
| `calc_basis/08_部材終局耐力/04_RC_部材_非線形復元力特性.md` | [nonlinear/05_RC_部材_非線形復元力特性.md](../../docs/nonlinear/05_RC_部材_非線形復元力特性.md) | 移動・全節維持 |
| `calc_basis/08_部材終局耐力/05_SRC_SRRC_梁_せん断終局.md` | [calc_basis/08_部材終局耐力/05_SRC_SRRC_梁_せん断終局.md](../../docs/calc_basis/08_部材終局耐力/05_SRC_SRRC_梁_せん断終局.md) | 維持・全節維持 |
| `calc_basis/08_部材終局耐力/06_CFT_柱_終局耐力.md` | [calc_basis/08_部材終局耐力/06_CFT_柱_終局耐力.md](../../docs/calc_basis/08_部材終局耐力/06_CFT_柱_終局耐力.md) | 維持・全節維持 |
| `calc_basis/08_部材終局耐力/07_終局検定の統括.md` | [calc_basis/08_部材終局耐力/07_終局検定の統括.md](../../docs/calc_basis/08_部材終局耐力/07_終局検定の統括.md) | 維持・全節維持 |
| `calc_basis/08_部材終局耐力/README.md` | [calc_basis/08_部材終局耐力/README.md](../../docs/calc_basis/08_部材終局耐力/README.md) | 章入口・案内を更新 |
| `calc_basis/09_免震制振/01_免震アイソレータ.md` | [calc_basis/09_免震制振/01_免震アイソレータ.md](../../docs/calc_basis/09_免震制振/01_免震アイソレータ.md) | 分割（節対応参照） |
| `calc_basis/09_免震制振/02_制振ダンパー.md` | [calc_basis/09_免震制振/02_制振ダンパー.md](../../docs/calc_basis/09_免震制振/02_制振ダンパー.md) | 維持・全節維持 |
| `calc_basis/09_免震制振/03_座屈拘束ブレース.md` | [calc_basis/06_一次設計/06_座屈拘束ブレース.md](../../docs/calc_basis/06_一次設計/06_座屈拘束ブレース.md) | 移動・全節維持 |
| `calc_basis/09_免震制振/04_辻_山田モデル.md` | [calc_basis/09_免震制振/04_辻_山田モデル.md](../../docs/calc_basis/09_免震制振/04_辻_山田モデル.md) | 維持・全節維持 |
| `calc_basis/09_免震制振/README.md` | [calc_basis/09_免震制振/README.md](../../docs/calc_basis/09_免震制振/README.md) | 章入口・案内を更新 |
| `calc_basis/10_数量積算/01_数量積算の算定式.md` | [calc_basis/10_数量積算/01_数量積算の算定式.md](../../docs/calc_basis/10_数量積算/01_数量積算の算定式.md) | 維持・全節維持 |
| `calc_basis/10_数量積算/README.md` | [calc_basis/10_数量積算/README.md](../../docs/calc_basis/10_数量積算/README.md) | 章入口・案内を更新 |
| `calc_basis/README.md` | [calc_basis/README.md](../../docs/calc_basis/README.md) | 章入口・案内を更新 |
| `introduction.md` | [introduction.md](../../docs/introduction.md) | 章入口・案内を更新 |
| `mcp_server/01_ビルドと起動.md` | [mcp_server/01_ビルドと起動.md](../../docs/mcp_server/01_ビルドと起動.md) | 維持・全節維持 |
| `mcp_server/02_クライアント設定例.md` | [mcp_server/02_クライアント設定例.md](../../docs/mcp_server/02_クライアント設定例.md) | 維持・全節維持 |
| `mcp_server/03_ツール一覧.md` | [mcp_server/03_ツール一覧.md](../../docs/mcp_server/03_ツール一覧.md) | 維持・全節維持 |
| `mcp_server/04_解析ジョブと結果取得.md` | [mcp_server/04_解析ジョブと結果取得.md](../../docs/mcp_server/04_解析ジョブと結果取得.md) | 維持・全節維持 |
| `mcp_server/05_制約・注意点.md` | [mcp_server/05_制約・注意点.md](../../docs/mcp_server/05_制約・注意点.md) | 維持・全節維持 |
| `mcp_server/README.md` | [mcp_server/README.md](../../docs/mcp_server/README.md) | 章入口・案内を更新 |
| `model_edit/01_通り芯.md` | [model_edit/01_通り芯.md](../../docs/model_edit/01_通り芯.md) | 維持・全節維持 |
| `model_edit/02_断面の符号と階.md` | [model_edit/02_断面の符号と階.md](../../docs/model_edit/02_断面の符号と階.md) | 維持・全節維持 |
| `model_edit/03_架構作成ウィザード.md` | [model_edit/03_架構作成ウィザード.md](../../docs/model_edit/03_架構作成ウィザード.md) | 維持・全節維持 |
| `model_edit/04_立体グリッドとスナップ.md` | [model_edit/04_立体グリッドとスナップ.md](../../docs/model_edit/04_立体グリッドとスナップ.md) | 維持・全節維持 |
| `model_edit/05_階への複製.md` | [model_edit/05_階への複製.md](../../docs/model_edit/05_階への複製.md) | 維持・全節維持 |
| `model_edit/06_画面構成と工程タブ.md` | [model_edit/06_画面構成と工程タブ.md](../../docs/model_edit/06_画面構成と工程タブ.md) | 維持・全節維持 |
| `model_edit/07_テーブル編集とグリッド操作.md` | [model_edit/07_テーブル編集とグリッド操作.md](../../docs/model_edit/07_テーブル編集とグリッド操作.md) | 維持・全節維持 |
| `model_edit/08_断面用途.md` | [model_edit/08_断面用途.md](../../docs/model_edit/08_断面用途.md) | 維持・全節維持 |
| `model_edit/09_梁の鉛直偏心.md` | [model_edit/09_梁の鉛直偏心.md](../../docs/model_edit/09_梁の鉛直偏心.md) | 維持・全節維持 |
| `model_edit/10_断面案の検討.md` | [section_study/01_断面案の検討.md](../../docs/section_study/01_断面案の検討.md) | 移動・全節維持 |
| `model_edit/README.md` | [model_edit/README.md](../../docs/model_edit/README.md) | 章入口・案内を更新 |
| `model_io/01_プロジェクト形式_ovika.md` | [model_io/01_プロジェクト形式_ovika.md](../../docs/model_io/01_プロジェクト形式_ovika.md) | 維持・全節維持 |
| `model_io/02_ST-Bridge_形式.md` | [model_io/02_ST-Bridge_形式.md](../../docs/model_io/02_ST-Bridge_形式.md) | 維持・全節維持 |
| `model_io/03_ST-Bridge_要素別変換状況.md` | [model_io/03_ST-Bridge_要素別変換状況.md](../../docs/model_io/03_ST-Bridge_要素別変換状況.md) | 維持・全節維持 |
| `model_io/04_波形ライブラリ.md` | [model_io/04_波形ライブラリ.md](../../docs/model_io/04_波形ライブラリ.md) | 維持・全節維持 |
| `model_io/README.md` | [model_io/README.md](../../docs/model_io/README.md) | 章入口・案内を更新 |
| `preparation/01_階の分布.md` | [preparation/01_階の分布.md](../../docs/preparation/01_階の分布.md) | 維持・全節維持 |
| `preparation/02_地震力_Ai分布.md` | [preparation/02_地震力_Ai分布.md](../../docs/preparation/02_地震力_Ai分布.md) | 維持・全節維持 |
| `preparation/04_剛域.md` | [preparation/04_剛域.md](../../docs/preparation/04_剛域.md) | 維持・全節維持 |
| `preparation/05_ねじり解放.md` | [preparation/05_ねじり解放.md](../../docs/preparation/05_ねじり解放.md) | 維持・全節維持 |
| `preparation/06_仕口パネル.md` | [preparation/06_仕口パネル.md](../../docs/preparation/06_仕口パネル.md) | 維持・全節維持 |
| `preparation/07_断面性能.md` | [preparation/07_断面性能.md](../../docs/preparation/07_断面性能.md) | 維持・全節維持 |
| `preparation/08_幅厚比.md` | [preparation/08_幅厚比.md](../../docs/preparation/08_幅厚比.md) | 維持・全節維持 |
| `preparation/09_部材剛性.md` | [preparation/09_部材剛性.md](../../docs/preparation/09_部材剛性.md) | 維持・全節維持 |
| `preparation/10_荷重集計.md` | [preparation/10_荷重集計.md](../../docs/preparation/10_荷重集計.md) | 維持・全節維持 |
| `preparation/11_出力と保存.md` | [preparation/11_出力と保存.md](../../docs/preparation/11_出力と保存.md) | 維持・全節維持 |
| `preparation/README.md` | [preparation/README.md](../../docs/preparation/README.md) | 章入口・案内を更新 |
| `result_view/01_変形図の表示倍率.md` | [result_view/01_変形図の表示倍率.md](../../docs/result_view/01_変形図の表示倍率.md) | 維持・全節維持 |
| `result_view/02_梁の内部たわみ表示.md` | [result_view/02_梁の内部たわみ表示.md](../../docs/result_view/02_梁の内部たわみ表示.md) | 維持・全節維持 |
| `result_view/03_応力図への変形の重ね表示.md` | [result_view/03_応力図への変形の重ね表示.md](../../docs/result_view/03_応力図への変形の重ね表示.md) | 維持・全節維持 |
| `result_view/04_床・二次部材の変形追従.md` | [result_view/04_床・二次部材の変形追従.md](../../docs/result_view/04_床・二次部材の変形追従.md) | 維持・全節維持 |
| `result_view/05_支持条件・剛床マークの表示.md` | [result_view/05_支持条件・剛床マークの表示.md](../../docs/result_view/05_支持条件・剛床マークの表示.md) | 維持・全節維持 |
| `result_view/06_検定比の表示.md` | [result_view/06_検定比の表示.md](../../docs/result_view/06_検定比の表示.md) | 維持・全節維持 |
| `result_view/07_モデル化の表示.md` | [result_view/07_モデル化の表示.md](../../docs/result_view/07_モデル化の表示.md) | 維持・全節維持 |
| `result_view/08_応力図の曲線補間.md` | [result_view/08_応力図の曲線補間.md](../../docs/result_view/08_応力図の曲線補間.md) | 維持・全節維持 |
| `result_view/09_ヒンジ図の表示.md` | [result_view/09_ヒンジ図の表示.md](../../docs/result_view/09_ヒンジ図の表示.md) | 維持・全節維持 |
| `result_view/10_時刻歴アニメーションの表示.md` | [result_view/10_時刻歴アニメーションの表示.md](../../docs/result_view/10_時刻歴アニメーションの表示.md) | 維持・全節維持 |
| `result_view/11_層応答分布グラフの表示.md` | [result_view/11_層応答分布グラフの表示.md](../../docs/result_view/11_層応答分布グラフの表示.md) | 維持・全節維持 |
| `result_view/12_構面の表示.md` | [result_view/12_構面の表示.md](../../docs/result_view/12_構面の表示.md) | 維持・全節維持 |
| `result_view/13_質点系の3D表示.md` | [result_view/13_質点系の3D表示.md](../../docs/result_view/13_質点系の3D表示.md) | 維持・全節維持 |
| `result_view/14_ナビゲータの解析結果ツリー.md` | [result_view/14_ナビゲータの解析結果ツリー.md](../../docs/result_view/14_ナビゲータの解析結果ツリー.md) | 維持・全節維持 |
| `result_view/15_床板・壁版の表示.md` | [result_view/15_床板・壁版の表示.md](../../docs/result_view/15_床板・壁版の表示.md) | 維持・全節維持 |
| `result_view/README.md` | [result_view/README.md](../../docs/result_view/README.md) | 章入口・案内を更新 |

## 責務の境界

| 利用目的 | 正本への入口 |
| --- | --- |
| 基本操作・モデル入力 | [model_edit/README.md](../../docs/model_edit/README.md) |
| 準備・解析実行・結果確認 | [workflow/README.md](../../docs/workflow/README.md) |
| 断面案の検討 | [section_study/README.md](../../docs/section_study/README.md) |
| 荷重・重量・質量 | [calc_basis/01_荷重/README.md](../../docs/calc_basis/01_荷重/README.md) |
| 材料・断面諸量・弾性剛性 | [elastic/README.md](../../docs/elastic/README.md) |
| 非線形モデル・復元力 | [nonlinear/README.md](../../docs/nonlinear/README.md) |
| 構造解析の方法 | [calc_basis/05_構造解析/README.md](../../docs/calc_basis/05_構造解析/README.md) |
| 許容応力度・部材検定 | [calc_basis/06_一次設計/README.md](../../docs/calc_basis/06_一次設計/README.md) |
| 部材終局耐力 | [calc_basis/08_部材終局耐力/README.md](../../docs/calc_basis/08_部材終局耐力/README.md) |
| 建物全体の耐震評価 | [calc_basis/07_二次設計/README.md](../../docs/calc_basis/07_二次設計/README.md) |
| 数量積算 | [calc_basis/10_数量積算/README.md](../../docs/calc_basis/10_数量積算/README.md) |
| 保存・入出力・外部AI連携 | [external/README.md](../../docs/external/README.md) |

## 外部AI連携の正本境界

MCPの6ページは本再編では配置を維持する。GUIから使う接続・許可・検討操作は製品マニュアルを正本とする。独立起動とAPI開発契約をまとめる既存文書はないため、分割先を `dev_docs/mcp_server.md` とし、以下の見出しを使用する。同ファイルは未作成であり、リンクにはしていない。本文の分離は [Issue #1030](https://github.com/hrntsm/SEPIKA/issues/1030) の対象で、ここでは配置と責務だけを定める。

| 現在のページ・節 | 製品側に保持する正本 | 開発側の正本先（未作成）・既存の実装入口 |
| --- | --- | --- |
| `mcp_server/README.md`：概要・機能フラグ・この章の内容 | [外部AI連携の入口](../../docs/mcp_server/README.md)：GUI接続の入口と公開操作の概要 | `dev_docs/mcp_server.md#構成と起動方式`：stdio、機能フラグ、独立サーバの構成。[クレート構成](../architecture.md) |
| `01_ビルドと起動.md`：開いているGUIの検討への接続 | [GUI接続の起動手順](../../docs/mcp_server/01_ビルドと起動.md#開いているguiの検討への接続) | GUIブリッジの通信実装は [gui_bridge.rs](../../crates/sepika-mcp/src/gui_bridge.rs) |
| `01_ビルドと起動.md`：独立したモデルでの起動・環境変数・stdout・起動エラー | GUIの接続方式との違いだけを参照する | `dev_docs/mcp_server.md#ビルドと独立起動`。[main.rs](../../crates/sepika-mcp/src/main.rs)、[Cargo.toml](../../crates/sepika-mcp/Cargo.toml) |
| `02_クライアント設定例.md`：開いているGUIへ接続する場合 | [GUI接続設定](../../docs/mcp_server/02_クライアント設定例.md#開いているguiへ接続する場合) | 独立サーバの設定と混在させない |
| `02_クライアント設定例.md`：Claude Code・Claude Desktop | GUI接続の設定例から独立起動の開発文書へ案内する | `dev_docs/mcp_server.md#独立起動クライアント設定`：両クライアントの独立起動設定 |
| `03_ツール一覧.md`：開いているGUIへの接続 | [GUIで公開する検討操作](../../docs/mcp_server/03_ツール一覧.md#開いているguiへの接続) | GUIの要求・応答型は [sepika-ai-connection](../../crates/sepika-ai-connection/src/lib.rs) |
| `03_ツール一覧.md`：model_query・model_edit（全下位節）・quantity_takeoff・analysis_run・analysis_status・result_get | GUIの公開範囲と利用者が確認する結果の意味を参照する | `dev_docs/mcp_server.md#独立起動ツール契約` 以下に同名ツールの小見出し。[server.rs](../../crates/sepika-mcp/src/server.rs)、[query.rs](../../crates/sepika-mcp/src/query.rs)、[edit.rs](../../crates/sepika-mcp/src/edit.rs)、[JobParams](../../crates/sepika-mcp/src/job/mod.rs) |
| `04_解析ジョブと結果取得.md`：解析ジョブのフロー | GUIの実行と結果確認は [準備・解析の実行・結果の確認](../../docs/workflow/README.md) | `dev_docs/mcp_server.md#ジョブと結果取得`。[JobRegistry・JobStatus](../../crates/sepika-mcp/src/lib.rs) |
| `04_解析ジョブと結果取得.md`：保存成果の生成元と利用可否 | OViKAの保存成果の扱いは [保存結果の計算契約と再実行](../../docs/model_io/01_プロジェクト形式_ovika.md#保存結果の計算契約と再実行) | `dev_docs/mcp_server.md#保存成果の生成元と利用可否`：result_getのstatus/entry/provenance/reason契約。[persist.rs](../../crates/sepika-mcp/src/persist.rs) |
| `04_解析ジョブと結果取得.md`：結果ストアのスキーマ（NodalDisp・MemberForce・Modal・TimeHistory） | GUIで読む量・単位は各結果表示ページを参照する | `dev_docs/mcp_server.md#結果ストアのスキーマ`。IOの公開型は [results/mod.rs](../../crates/sepika-io/src/results/mod.rs)、[APIリファレンス](https://hrntsm.github.io/SEPIKA/api/) |
| `05_制約・注意点.md`：GUI接続の共有と許可・保持結果・提出・要求ID | [GUI接続の共有と許可](../../docs/mcp_server/05_制約・注意点.md#gui接続の共有と許可) | GUI通信・受理台帳の型の契約は [sepika-ai-connection](../../crates/sepika-ai-connection/src/lib.rs) |
| `05_制約・注意点.md`：独立モデル読込・編集範囲・2節点条件・質点系対象外・ジョブの永続化制限・DesignCheck応答 | 入力不足の構造計算上の影響は各モデル入力・解析の正本を参照する | `dev_docs/mcp_server.md#独立起動のモデル入力制約` と上記ツール/保存成果の節。実装入口は [main.rs](../../crates/sepika-mcp/src/main.rs)、[server.rs](../../crates/sepika-mcp/src/server.rs)、[persist.rs](../../crates/sepika-mcp/src/persist.rs) |

## 分割ページの節対応

見出しのない式・表は直前の節に含める。移動した内容は移動先への案内に置き換え、同じ計算条件を再定義しない。冒頭の契約も下表に対応を示す。

### calc_basis/01_荷重/07_荷重の入力.md

| 元の節・範囲 | 正本先 |
| --- | --- |
| 冒頭（最初の節より前） | [荷重入力の操作](../../docs/model_edit/11_荷重の入力.md) |
| 荷重ケースの構成 | [model_edit/11_荷重の入力.md](../../docs/model_edit/11_荷重の入力.md) |
| 振動荷重ケースとの違い | [model_edit/11_荷重の入力.md](../../docs/model_edit/11_荷重の入力.md) |
| 節点荷重 | [calc_basis/01_荷重/07_荷重の入力.md](../../docs/calc_basis/01_荷重/07_荷重の入力.md) |
| 1 つの節点に何件でも定義できます。 | [calc_basis/01_荷重/07_荷重の入力.md](../../docs/calc_basis/01_荷重/07_荷重の入力.md) |
| 部材荷重 | [calc_basis/01_荷重/07_荷重の入力.md](../../docs/calc_basis/01_荷重/07_荷重の入力.md)、[model_edit/11_荷重の入力.md](../../docs/model_edit/11_荷重の入力.md) |
| 作用方向の計算条件 | [calc_basis/01_荷重/07_荷重の入力.md](../../docs/calc_basis/01_荷重/07_荷重の入力.md) |
| 作用位置と区間 | [calc_basis/01_荷重/07_荷重の入力.md](../../docs/calc_basis/01_荷重/07_荷重の入力.md) |
| 節点の座標編集と作用区間 | [calc_basis/01_荷重/07_荷重の入力.md](../../docs/calc_basis/01_荷重/07_荷重の入力.md) |
| 荷重を載せられる部材 | [calc_basis/01_荷重/07_荷重の入力.md](../../docs/calc_basis/01_荷重/07_荷重の入力.md) |
| ブレースに載せる荷重 | [calc_basis/01_荷重/07_荷重の入力.md](../../docs/calc_basis/01_荷重/07_荷重の入力.md) |
| 準備計算が生成する荷重との区別 | [model_edit/11_荷重の入力.md](../../docs/model_edit/11_荷重の入力.md) |
| 荷重の名称 | [model_edit/11_荷重の入力.md](../../docs/model_edit/11_荷重の入力.md) |
| 対象の節点・部材の選び方 | [model_edit/11_荷重の入力.md](../../docs/model_edit/11_荷重の入力.md) |

### calc_basis/01_荷重/08_床の断面と自重.md

| 元の節・範囲 | 正本先 |
| --- | --- |
| 冒頭（最初の節より前） | 重量側に断面と自重の関係を維持。操作への参照を追加 |
| 床板の鉛直配置 | [model_edit/12_床板の入力.md](../../docs/model_edit/12_床板の入力.md) |
| 板厚と自重は断面から決まる | [calc_basis/01_荷重/08_床の断面と自重.md](../../docs/calc_basis/01_荷重/08_床の断面と自重.md) |
| 鋼床板の設計自重と密度 | [calc_basis/01_荷重/08_床の断面と自重.md](../../docs/calc_basis/01_荷重/08_床の断面と自重.md) |
| 床板の物理質量と適用モード | [calc_basis/01_荷重/08_床の断面と自重.md](../../docs/calc_basis/01_荷重/08_床の断面と自重.md) |
| 断面が未割当の床は解析前チェックで止まる | [calc_basis/01_荷重/08_床の断面と自重.md](../../docs/calc_basis/01_荷重/08_床の断面と自重.md) |
| 床の断面を作る | [model_edit/12_床板の入力.md](../../docs/model_edit/12_床板の入力.md) |
| ST-Bridge との対応 | [calc_basis/01_荷重/08_床の断面と自重.md](../../docs/calc_basis/01_荷重/08_床の断面と自重.md) |

### calc_basis/01_荷重/09_壁の断面と自重.md

| 元の節・範囲 | 正本先 |
| --- | --- |
| 冒頭（最初の節より前） | 重量側に共通総量と未算定条件を維持。操作への参照を追加 |
| 板厚と自重は断面から決まる | [calc_basis/01_荷重/09_壁の断面と自重.md](../../docs/calc_basis/01_荷重/09_壁の断面と自重.md) |
| 仕上げ・増打ちは面荷重として入力する | [calc_basis/01_荷重/09_壁の断面と自重.md](../../docs/calc_basis/01_荷重/09_壁の断面と自重.md) |
| 壁版を作る | [model_edit/13_壁版の入力.md](../../docs/model_edit/13_壁版の入力.md) |
| 耐震スリット | [calc_basis/01_荷重/09_壁の断面と自重.md](../../docs/calc_basis/01_荷重/09_壁の断面と自重.md) |
| 三方スリット壁と垂れ壁は別物です。 | [calc_basis/01_荷重/09_壁の断面と自重.md](../../docs/calc_basis/01_荷重/09_壁の断面と自重.md) |
| 剛性は、切れていない辺の部材にだけ算入します。 | [calc_basis/01_荷重/09_壁の断面と自重.md](../../docs/calc_basis/01_荷重/09_壁の断面と自重.md) |
| 自重は、切れていない支持辺へ伝えます。 | [calc_basis/01_荷重/09_壁の断面と自重.md](../../docs/calc_basis/01_荷重/09_壁の断面と自重.md) |
| 自重の支持先とスリット | [calc_basis/01_荷重/09_壁の断面と自重.md](../../docs/calc_basis/01_荷重/09_壁の断面と自重.md) |
| 上下の梁際をともに切った壁版（境界が 4 節点の囲まれた壁版）は、実際の納まりとして想定しないため、負担率の指定内容や柱際の状態によらず解析前チェックのエラーとします。 | [calc_basis/01_荷重/09_壁の断面と自重.md](../../docs/calc_basis/01_荷重/09_壁の断面と自重.md) |
| スリットを指定する辺の指し方 | [model_edit/13_壁版の入力.md](../../docs/model_edit/13_壁版の入力.md) |
| スリットを指定できるのは、境界が 4 節点の壁版だけです。 | [model_edit/13_壁版の入力.md](../../docs/model_edit/13_壁版の入力.md) |
| 取り付く壁版の取付き先 | [model_edit/13_壁版の入力.md](../../docs/model_edit/13_壁版の入力.md) |
| 自立壁の高さは階高いっぱいにできる | [model_edit/13_壁版の入力.md](../../docs/model_edit/13_壁版の入力.md) |
| 階高を選べるのは自立壁だけです。 | [model_edit/13_壁版の入力.md](../../docs/model_edit/13_壁版の入力.md) |
| 取り付く壁版への自動変換 | [calc_basis/01_荷重/09_壁の断面と自重.md](../../docs/calc_basis/01_荷重/09_壁の断面と自重.md) |
| ST-Bridge との対応 | [calc_basis/01_荷重/09_壁の断面と自重.md](../../docs/calc_basis/01_荷重/09_壁の断面と自重.md) |
| 採用範囲と確認限界 | [calc_basis/01_荷重/09_壁の断面と自重.md](../../docs/calc_basis/01_荷重/09_壁の断面と自重.md) |

### calc_basis/04_要素剛性/05_壁エレメントモデル.md

| 元の節・範囲 | 正本先 |
| --- | --- |
| 冒頭（最初の節より前） | 弾性側に壁要素の成立・幾何・目的別適用範囲を維持 |
| 壁要素の幾何 | [calc_basis/04_要素剛性/05_壁エレメントモデル.md](../../docs/calc_basis/04_要素剛性/05_壁エレメントモデル.md) |
| 耐震壁として扱う条件 | [calc_basis/04_要素剛性/05_壁エレメントモデル.md](../../docs/calc_basis/04_要素剛性/05_壁エレメントモデル.md) |
| 目的別の適用範囲 | [calc_basis/04_要素剛性/05_壁エレメントモデル.md](../../docs/calc_basis/04_要素剛性/05_壁エレメントモデル.md) |
| 壁柱・側柱・上下梁の分担 | [calc_basis/04_要素剛性/05_壁エレメントモデル.md](../../docs/calc_basis/04_要素剛性/05_壁エレメントモデル.md) |
| 側柱の回転解放と適用範囲 | [calc_basis/04_要素剛性/05_壁エレメントモデル.md](../../docs/calc_basis/04_要素剛性/05_壁エレメントモデル.md)、[nonlinear/06_壁の非線形モデル.md](../../docs/nonlinear/06_壁の非線形モデル.md) |
| 壁上下の剛梁の役割 | [calc_basis/04_要素剛性/05_壁エレメントモデル.md](../../docs/calc_basis/04_要素剛性/05_壁エレメントモデル.md) |
| 壁と周辺架構の構造種別 | [calc_basis/04_要素剛性/05_壁エレメントモデル.md](../../docs/calc_basis/04_要素剛性/05_壁エレメントモデル.md) |
| 算定式 | [calc_basis/04_要素剛性/05_壁エレメントモデル.md](../../docs/calc_basis/04_要素剛性/05_壁エレメントモデル.md) |
| 記号・単位 | [calc_basis/04_要素剛性/05_壁エレメントモデル.md](../../docs/calc_basis/04_要素剛性/05_壁エレメントモデル.md) |
| 面内せん断の非線形性 | [nonlinear/06_壁の非線形モデル.md](../../docs/nonlinear/06_壁の非線形モデル.md) |
| 軸・曲げの非線形性（ファイバー壁柱） | [nonlinear/06_壁の非線形モデル.md](../../docs/nonlinear/06_壁の非線形モデル.md) |
| 鋼板耐震壁の終局せん断強度 | [calc_basis/08_部材終局耐力/09_鋼板耐震壁のせん断耐力.md](../../docs/calc_basis/08_部材終局耐力/09_鋼板耐震壁のせん断耐力.md) |
| 設定不備による停止 | [nonlinear/06_壁の非線形モデル.md](../../docs/nonlinear/06_壁の非線形モデル.md) |
| 壁単体の確定ステップ応答 | [result_view/16_壁の確定ステップ応答.md](../../docs/result_view/16_壁の確定ステップ応答.md) |
| 力と変形の根拠 | [result_view/16_壁の確定ステップ応答.md](../../docs/result_view/16_壁の確定ステップ応答.md) |
| 質量 | [calc_basis/04_要素剛性/05_壁エレメントモデル.md](../../docs/calc_basis/04_要素剛性/05_壁エレメントモデル.md) |
| 側柱の回転解放：幾何と保持自由度 | [calc_basis/04_要素剛性/05_壁エレメントモデル.md](../../docs/calc_basis/04_要素剛性/05_壁エレメントモデル.md)（接続の定義） |
| 側柱の回転解放：材料履歴・局所釣合い・解析別適用表・共有柱・診断 | [nonlinear/06_壁の非線形モデル.md](../../docs/nonlinear/06_壁の非線形モデル.md)「側柱の非線形接続と適用範囲」 |
| 非線形時刻歴の壁柱断面力の記録 | [result_view/16_壁の確定ステップ応答.md](../../docs/result_view/16_壁の確定ステップ応答.md) |

### calc_basis/04_要素剛性/06_パネルゾーン.md

| 元の節・範囲 | 正本先 |
| --- | --- |
| 冒頭（最初の節より前） | 弾性側に記号・モデル化対象・幾何・座標変換を維持 |
| 記号・単位 | [calc_basis/04_要素剛性/06_パネルゾーン.md](../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md) |
| モデル化の対象と既定 | [calc_basis/04_要素剛性/06_パネルゾーン.md](../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md) |
| 仕口パネル要素のモデル化対象 | [calc_basis/04_要素剛性/06_パネルゾーン.md](../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md) |
| 仕口パネル要素の剛性 | [calc_basis/04_要素剛性/06_パネルゾーン.md](../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md) |
| 柱主軸と全体座標の変換 | [calc_basis/04_要素剛性/06_パネルゾーン.md](../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md) |
| 部材端の接合面とオフセット | [calc_basis/04_要素剛性/06_パネルゾーン.md](../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md) |
| 仕口パネル要素の寸法と実効体積 | [calc_basis/04_要素剛性/06_パネルゾーン.md](../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md) |
| H 形柱の弱軸側の近似 | [calc_basis/04_要素剛性/06_パネルゾーン.md](../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md) |
| 標準形式の適用範囲 | [calc_basis/04_要素剛性/06_パネルゾーン.md](../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md) |
| 部材との接合位置 | [calc_basis/04_要素剛性/06_パネルゾーン.md](../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md) |
| 仕口パネル要素（S造）の弾塑性 | [nonlinear/07_仕口パネルの復元力.md](../../docs/nonlinear/07_仕口パネルの復元力.md) |
| 非線形応答の利用上の制限 | [nonlinear/07_仕口パネルの復元力.md](../../docs/nonlinear/07_仕口パネルの復元力.md) |
| 質量 | [calc_basis/04_要素剛性/06_パネルゾーン.md](../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md) |
| 柱梁接合部の断面算定 | [calc_basis/04_要素剛性/06_パネルゾーン.md](../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md) |
| 諸元の確認 | [calc_basis/04_要素剛性/06_パネルゾーン.md](../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md) |

### calc_basis/04_要素剛性/07_ばね要素.md

| 元の節・範囲 | 正本先 |
| --- | --- |
| 冒頭（最初の節より前） | 弾性ばねと非線形ばねの目的別入口へ置換。製品固有係数の適用条件は非線形側に維持 |
| 算定式 | [calc_basis/04_要素剛性/07_ばね要素.md](../../docs/calc_basis/04_要素剛性/07_ばね要素.md) |
| 記号・単位 | [calc_basis/04_要素剛性/07_ばね要素.md](../../docs/calc_basis/04_要素剛性/07_ばね要素.md)、[nonlinear/08_免震制振ばねの復元力.md](../../docs/nonlinear/08_免震制振ばねの復元力.md) |
| 算定式：積層ゴム・摩擦・Maxwell・免震回転剛性/残留せん断剛性 | [nonlinear/08_免震制振ばねの復元力.md](../../docs/nonlinear/08_免震制振ばねの復元力.md) |
| 算定式：節点ばね・支点ばね | [calc_basis/04_要素剛性/07_ばね要素.md](../../docs/calc_basis/04_要素剛性/07_ばね要素.md) |
| 記号・単位表、実装参照 | 対応する式ごとに両ページへ分配 |

### calc_basis/09_免震制振/01_免震アイソレータ.md

| 元の節・範囲 | 正本先 |
| --- | --- |
| 冒頭（最初の節より前） | 免震特性の記号・単位を維持 |
| 記号・単位 | [calc_basis/09_免震制振/01_免震アイソレータ.md](../../docs/calc_basis/09_免震制振/01_免震アイソレータ.md) |
| マルチシア（多層積層ゴム）低減率 | [calc_basis/09_免震制振/01_免震アイソレータ.md](../../docs/calc_basis/09_免震制振/01_免震アイソレータ.md) |
| 算定式 | [calc_basis/09_免震制振/01_免震アイソレータ.md](../../docs/calc_basis/09_免震制振/01_免震アイソレータ.md) |
| 摩擦（すべり）支承 | [calc_basis/09_免震制振/01_免震アイソレータ.md](../../docs/calc_basis/09_免震制振/01_免震アイソレータ.md) |
| 鉛プラグ入り積層ゴム（LRB）統一型・歪依存 | [calc_basis/09_免震制振/01_免震アイソレータ.md](../../docs/calc_basis/09_免震制振/01_免震アイソレータ.md) |
| 算定式 | [calc_basis/09_免震制振/01_免震アイソレータ.md](../../docs/calc_basis/09_免震制振/01_免震アイソレータ.md) |
| 転がり支承・球面すべり支承・高減衰ゴム | [calc_basis/09_免震制振/01_免震アイソレータ.md](../../docs/calc_basis/09_免震制振/01_免震アイソレータ.md) |
| 算定式（代表） | [calc_basis/09_免震制振/01_免震アイソレータ.md](../../docs/calc_basis/09_免震制振/01_免震アイソレータ.md) |
| 支点への免震支承配置 | [model_edit/14_免震支承の配置.md](../../docs/model_edit/14_免震支承の配置.md) |

