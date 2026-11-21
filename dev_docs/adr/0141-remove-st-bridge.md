# ST-Bridge の入出力と専用情報の保持を廃止する

Status: accepted

## 判断

[Issue #1738](https://github.com/hrntsm/SEPIKA/issues/1738) により、ST-Bridge の読込・書出を公開 API と GUI から削除する。交換のために保持していた元配筋、強度指定、明示原階と所属、標準節点 ID/GUID、組立断面の来歴、および専用の編集・診断・解析準備を廃止する。交換仕様の再導入は別の設計判断とする。

通常の階・節点所属、材料・配筋・断面の割当、部材の用途・端部偏心と各種計算は、本体の入力データで扱う。主材料・主筋・せん断補強筋・内蔵鉄骨／鋼管の材料は断面の割当を参照する。従来の交換入力を使っていた本体の回帰試験は、通常のモデル構築かネイティブ入力で維持する。

開発中の OViKA 保存スキーマは [ADR 0014](0014-schema-compatibility.md) に従う。入力モデルの最上位または階データに未定義の項目があれば読込エラーにする。過去の強度・所属等の指定を失ったモデルが解析・検定に進むことを防ぐ。

## 理由

本体の仕様を整える段階にあり、交換形式の対応関係と変換契約を同時に維持すると、材料・階・断面などの入力判断を本体と交換用の両方で管理する必要が生じる。交換に固有の責務を削除することで、本体の入力から計算までの経路に開発と検証を集中できる。

## 既存 ADR の適用範囲

過去の ADR に含まれる ST-Bridge 専用の操作・保存・変換・診断の記述は廃止前の判断を記録する。主架構用途と二次部材種別の区別、通常の階と剛床の分離、鉄筋呼び名と断面材料の割当、断面形状・部材配置の契約は引き継ぐ。[ADR 0004](0004-story-and-diaphragm-separation.md)、[ADR 0034](0034-rc-section-purpose-types-and-real-rebar.md)、[ADR 0036](0036-stbridge-section-use-explicit-mapping.md)、[ADR 0106](0106-beam-end-offsets.md)、[ADR 0119](0119-standard-rebar-designation.md)

計算時入力と検討案の照合では、交換用節点の採番・正規化を廃止する。主架構・二次部材・割当領域の識別と、OVIKA の保存・読込時の参照検証は引き継ぐ。[ADR 0088](0088-version-scoped-references.md)、[ADR 0091](0091-study-comparison-evidence.md)、[ADR 0092](0092-confirmed-study-adoption.md)
