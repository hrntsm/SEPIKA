# 荷重文書の整理記録（Issue #1019）

作成日: 2026-10-31

製品本文から内部構造・呼出条件・差分の履歴を分離した記録。現在の計算条件は [荷重・重量・質量](../../docs/calc_basis/01_荷重/README.md)を正とする。荷重分配の算法と積載荷重低減の実装は変更していない。

## 内部構造と再構築

- 囲まれた床板の境界は `FloorPlateAssignmentRegion` が持ち、床領域の `slab_ids` が帰属を保持する。境界所有の判断は [ADR 0020](../adr/0020-assign-plates-to-member-bounded-regions.md) を参照する。
- `Model::rebuild_floor_assignment_regions` は同じ境界の割当領域IDと割当状態を保持し、境界が変わったものは新IDの未設定領域へ置換する。`rebuild_floor_regions` による床領域の再構築ではIDが再生成される。旧新面積の相対差1e-3未満という照合は再構築報告用の判定で、荷重面積の近似ではない。
- 自立壁の伝達先床領域を保存IDに固定せず、現在の壁位置から求める。支持部材アンカーの解決と再構築の呼出位置は `crates/sepika-core/src/region_rebuild.rs`、`crates/sepika-io/src/stbridge/import/assemble.rs`、`crates/sepika-job/src/auto_loads.rs` が入口。

重複支持の判断は [ADR 0021](../adr/0021-same-position-support-members.md) に従う。

## 生成APIとGUIの境界

- 層生成APIの `Density` は材料自重を直接算入する。`SyncedGravityCases` は同期自重を識別して設計自重を物理質量へ置換する。`GravityCasesOnly` はケース内容だけを集計し、手入力を自重へ読み替えない。`generate_stories_with_synced_self_weight` の呼出前にDL自重同期が必要で、`generate_stories_with_opts` の `include_density_self_weight = false` は置換を行わない。通常GUIは自重同期済み経路を使う。入口は `crates/sepika-load/src/story_gen/generate.rs`。
- 多角形床の格子寸法・位相・辺別要求誤差と積分診断はAPI用で、GUIには設定・結果表示がない。製品本文には採用する数値積分条件と確認限界を残した。
- MCPの保存組合せ応答は単体保存結果とは異なり、`case: null`・`store.persisted: false`。自動G+P+Kや取得件数等のツール契約は `dev_docs/mcp_server.md` と実装を参照する。

## 符号付き反力の旧値比較

支間6000 mm、域外区間[7000,8000] mm、強度1/1 N/mmの旧重心比制限では0/1000 N、合力W・一次モーメントSによる値は−250/1250 Nとなる。これは旧新の数値比較で、域外荷重の物理的妥当性やMの端点閉合の検証ではない。[Issue #635](https://github.com/hrntsm/SEPIKA/issues/635) が域外入力の追跡先。

## 確認範囲

ソースを根拠に設計重量・物理質量・GUI操作の説明を整合した。法令・学会規準の原典照合を追加実施した記録ではない。[Issue #958](https://github.com/hrntsm/SEPIKA/issues/958) の原典確認と [Issue #1041](https://github.com/hrntsm/SEPIKA/issues/1041) の全体数式書式は本作業の対象外。
