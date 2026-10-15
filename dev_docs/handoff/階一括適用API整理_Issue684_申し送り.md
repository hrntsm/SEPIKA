# 階一括適用API整理（Issue #684）申し送り

作成日: 2026-10-16

対象: [Issue #684](https://github.com/hrntsm/SEPIKA/issues/684)、親 [#524](https://github.com/hrntsm/SEPIKA/issues/524)、発見 [#681](https://github.com/hrntsm/SEPIKA/issues/681)。
本書は調査と廃止判断の履歴。検証の入力・結果・限界は[専用V&V](../v_and_v/階一括適用API整理_Issue684_2026-10.md)を正とする。

## 問題と呼出所有

基点 `db1282251aadf585cb7b52e585df68164305527b` の公開 `ApplyStories` / `RestoreStories` は、階・節点・拘束・生成代表節点やダンパー等の記録を部分的に差し替え／復元するが、`SeismicWeightGeneration` を一緒に扱わなかった。
正常な生成記録付き2階モデルへ、別の正常な1階モデルから生成した結果を旧APIで採用すると、削除済み `StoryId(1)` が旧記録に残った。参照検査とOVIKA保存の拒否は #681 の正しい動作であり、検査を緩める理由にならない。

全Rustソースの旧API名検索では、実構築はEditの既存2試験、Appの `tests/source_story.rs`、ignored診断 `tests/perf_probe.rs` に限られた。`RestoreStories` は旧コマンド内部の逆操作生成だけだった。Appのコメント3か所の言及は実呼出ではない。外部クレート利用者の有無までは確認していない。
現用Appの生成は `generate_stories_action` から `sync_prepared_model` に入り、clone上の `prepare_model` と既存 `ApplyPreparedModel` で全Modelを採用する。この経路と正順Undo/Redoは正常だった。旧公開APIの再現を現在のGUI実障害や構造計算の数値誤差とは扱わない。

## 採用判断

production呼出のない旧2APIを廃止し、既存試験を全Model採用へ移す方針とした。
代案の部分snapshot拡張では、生成結果の `calculated_weights`、自動由来判定、入出力keyまで同時に扱う公開契約が必要となる。現用の全Model採用に加えて別の適用契約を維持する根拠がないため採用しない。互換wrapper、汎用snapshot基盤、推定修復、新依存も不要と判断した。

後方互換を維持しない方針は [AGENTS.md](../../AGENTS.md)、保存スキーマの扱いは [ADR 0014](../adr/0014-schema-compatibility.md) に従う。今回の軽微なAPI整理は[ADR作成の3条件](../adr/README.md#規約)を満たす新判断ではなく、新ADRを追加しない。製品計算機能・生成式・利用者の保持契約・保存スキーマを変更する判断ではないため、製品docsへ内部API説明を追加しない。新語彙や新しい計算近似もない。

## 移行で保持する契約

- Editの代表節点往復試験は、節点・階・拘束・`rep_nodes` / `generated_masters`・ダンパー記録を保持し、全Modelと地震重量生成記録の適用／Undo／Redoを検証する。質量方式試験の `CorrectedLumped` → `LumpedOnly` と復元も保持する。
- Appの生成結果採用は、既存 `apply_generated_weights` でcloneの生成出力と記録を一緒に更新し、`ApplyPreparedModel` をUndoStackへ積む。試験側へ旧適用実装や重量式を複製しない。
- 実App生成、正規 `DeleteStory`、再生成、逆順Undo2回、順方向Redo2回を検証する。再生成Undo直後の正常staleと、削除Undo後の元記録復元を区別する。
- 古いkeyや利用者変更済みCi・拘束・重量overrideは、typed参照が実在する正常対照として保持する。記録値と現用値の不一致を不在参照と同一視しない。
- ignored `perf_probe` の計測段階・説明を採用経路に合わせる。過去値と同条件の比較や、現用GUI全体の性能確認とは称さない。

## 調査結果の境界

未記録の別準備処理を途中へ挟み、過去のinverseだけを適用する試行では元記録へ戻らなかった。これはUndoStackの正規順序外であり、通常GUIの障害や別の是正根拠へ転用しない。
正常staleの保存成功も、鮮度があることや全計算入力が完成していることを意味しない。

## 記録時点と後続ゲート

旧API削除と恒久回帰の実装・直接検証を確認した。Edit移行2試験、App source_story13試験と新lifecycle2試験は成功し、新lifecycleでは別集合の16状態をOVIKA往復した。ignored診断はcompile成功・1 ignoredで計測未実施。初回Editの完成fixture識別不足による1失敗と修正、失敗時ソース未同時採取の限界も[専用V&V](../v_and_v/階一括適用API整理_Issue684_2026-10.md#是正後の恒久回帰と直接検証)に保持した。計算式・validator・生成機構・依存・スキーマ・UIは変更していない。
別担当の広域Edit・App検証は通常21スイート665成功、GUI22スイート1151成功、各失敗0・既存perf_probe 1 ignored。最新stable確認後のfmt、両構成全target Clippy、strict Rustdoc、製品docs検査、mdbook buildも成功した。構成間の件数を合算せず、性能計測・全workspaceローカル検証とは区別する。環境更新の初回権限エラーと許可再試行、証拠採取の限界も[専用V&V](../v_and_v/階一括適用API整理_Issue684_2026-10.md#windows広域静的文書検証)に記録した。
fresh独立レビュー、親監査、最終head全適用CI、通常mergeは本書で完了認定しない。[#684](https://github.com/hrntsm/SEPIKA/issues/684)の原3完了条件と公開PRで確認する。
native GUI手操作、外部API利用、非サンプル建物、数値・実験Validationは未確認である。#681の既存検証範囲と未実施範囲は変更しない。
