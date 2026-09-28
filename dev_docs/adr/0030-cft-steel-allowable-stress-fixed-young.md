# CFT の鋼管材料を専用参照で保持する

Status: accepted

## 決定

CFT の `Section.material` は充填コンクリート、`Section.steel_material` は鋼管材料として保持する。鋼管部分の物性は鋼管材料から、充填コンクリートの Fc・コンクリート種類は主材料から解決する。

## 背景と理由

CFT はコンクリートと鋼管の複合断面であり、単一の主材料では両領域の物性を表せない。ST-Bridge 取り込みでは `strength_concrete` を主材料、`StbSecSteelColumn_CFT_Same` の `strength` を鋼管材料へ分離して解決する。

## 影響

- `strength` がない入力は取り込みを継続するが、鋼管材料未設定のまま質量特性・自重の解決時にエラーとする。
- 質量特性は鋼管領域へ鋼管材料の密度、充填部へ主材料の Fc・コンクリート種類から求めた無筋コンクリート密度を適用する。

## 関連

- Issue #367
- [SRC / CFT 断面検定](../../docs/calc_basis/06_一次設計/04_SRC_CFT_断面検定.md)
