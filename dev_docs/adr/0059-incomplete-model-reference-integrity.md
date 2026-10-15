# 入力完成度と保存可能な参照ID整合性を分離する

- Status: accepted
- Date: 2026-10-16
- Related: [#678](https://github.com/hrntsm/SEPIKA/issues/678)、[ADR 0014](0014-schema-compatibility.md)、[ADR 0020](0020-assign-plates-to-member-bounded-regions.md)

## Context

断面検索の fallback が存在しても、保存された ID の添字契約や参照の実在は保証されない。
一方、全 `Model.validate` を保存条件にすると、主架構断面の用途未設定など、正常な入力途中の保存も拒否する。
未設定を保持できることと、別の実体や存在しない実体を参照できることを分ける必要がある。

## Decision

既存 `Model.validate` の構造 ID・参照検査だけを `validate_reference_ids` へ抽出して共用する。
OVIKA 保存・読込、`App::load_model` のモデル差し替え前、アプリ保存の成果同期前で適用する。
未割当 `None`、用途、幾何、入力完成度はこの検査へ追加せず、既存の個別検査を維持する。
GUI の `open_project_from` は従来の全 `Model.validate` を維持し、IO 単体の未完成受理とは区別する。

添字と ID の一致を要求する実体は Node・Elem・Section・Material・Story・FloorRegion・WallRegion・Slab・WallPlate の9種とする。
参照検査は既存の集合、すなわち要素の節点・断面、断面の材料4役、通り芯と床/壁領域の節点、床/壁領域の版、
版から既存 helper で得られる節点と断面、領域内外の二次部材の断面、一本部材の要素参照を維持する。
囲まれた版の境界が解決できない場合に helper が空を返す挙動まで含む抽出であり、新たな幾何・割当完成度の保証ではない。

二次部材 ID は全所属を横断する一意性を要求するが、非連番を許容する。
版の割当領域の安定 ID にも添字一致を要求せず、親 WallRegion を持たない有効な割当領域へ親必須条件を追加しない。
これらを添字 ID へ読み替える案は、[ADR 0020](0020-assign-plates-to-member-bounded-regions.md) の同一性を損なうため採用しない。

公開断面検索と二次部材の断面用途検査は、添字位置の同 ID 一致を優先し、その後に同 ID の先頭を選ぶ規則へ揃える。
この規則を不正 ID の保存許可とはしない。全 validate の保存への適用、ID 再採番・断面置換・削除・用途推定による救済は採用しない。

## Consequences

入力途中の通常保存と、構造 ID・参照不整合の拒否を両立する。参照不整合による失敗で既存ファイルやアプリのモデル・履歴・成果を変更しない。
不正な旧ファイルを自動修復する migration は追加せず、初回リリース前の互換方針は ADR 0014 に従う。
現在仕様は [OVIKA](../../docs/model_io/01_プロジェクト形式_ovika.md#未完成の入力と参照の整合性) を正とする。

一般参照の全網羅とはしない。Story 所属・Constraint/Mpc・荷重・生成記録等の未網羅集合は
[#681](https://github.com/hrntsm/SEPIKA/issues/681) が独立調査する。実到達や数値影響は静的な未網羅確認から推定しない。
CFT 二次用途の #539、RC 専用用途の #674、板参照の #677、STB 外部系列 ID の #571 はそれぞれの所有を維持する。
