# 建物指標のGUI非提供計算

製品の採用式・条件は[剛性率 Rs・偏心率 Re と形状係数 Fes](../docs/calc_basis/07_二次設計/04_形状係数_Fes.md)を正本とする。本書はGUIから選択できないD値略算の入口と式を扱う。GUIの偏心率でX／Y解析結果が不足したときに呼び出す代用計算ではない。

## D値による柱剛性の略算

`sepika_design_jp::secondary::eccentricity::d_value`（[core.rs](../crates/sepika-design-jp/src/secondary/eccentricity/core.rs)）は次の閉形式を提供する。

\\[ K_{c0}=12EI_c/h^3,\qquad k_c=I_c/h,\qquad \bar{k}=\frac{\sum(I_b/L_b)}{2k_c},\qquad D=aK_{c0} \\]

一般階は \\(a=\bar{k}/(2+\bar{k})\\)、柱脚固定の最下階は \\(a=(0.5+\bar{k})/(2+\bar{k})\\) とする。

| 記号 | 意味 | 単位 |
| --- | --- | --- |
| \\(D,K_{c0}\\) | 柱の水平剛性・標準剛性 | N/mm |
| \\(E\\) | ヤング係数 | N/mm² |
| \\(I_c,I_b\\) | 柱・梁の断面二次モーメント | mm⁴ |
| \\(h,L_b\\) | 層の上下端の高さの差・梁長 | mm |
| \\(k_c,I_b/L_b\\) | 柱・梁の剛比に用いる量 | mm³ |
| \\(a,\bar{k}\\) | D値の係数・梁柱の剛比 | - |

モデル抽出の入口は `sepika_design_jp::secondary::eccentricity`（[model_extract.rs](../crates/sepika-design-jp/src/secondary/eccentricity/model_extract.rs)）。中間節点で分割された鉛直材は連なりを1本の柱とみなし、層の上下端の高さの差をhとする。セグメントの材長をhに用いると12EI/h³が過大になるため、この扱いを必要とする。

## 文書配置の変更履歴

Issue #1026で、D値の式とGUI非提供範囲を製品本文から本書へ移した。計算の追加や採用値の変更は行っていない。部材群集計とDs表の正本は[構造特性係数 Ds](../docs/calc_basis/07_二次設計/02_構造特性係数_Ds.md)へ集約した。
