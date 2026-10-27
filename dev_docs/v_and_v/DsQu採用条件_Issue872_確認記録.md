# Ds・Qu採用条件の不足理由と合否保留（#872）

2026-10-27。[Issue #872](https://github.com/hrntsm/SEPIKA/issues/872)と追加された[#882原典照合](RC耐震ルート_Issue882_原典照合.md)の適用条件を対象とする。製品仕様は[保有水平耐力](../../docs/calc_basis/07_二次設計/01_保有水平耐力の判定.md#dsとquを採用する状態の条件)を正とする。状態🔶。既存の数値評価と採用条件の確認を分離するVerificationであり、実験Validation・専門家サインオフ・法規ルート全体の適合確認ではない。

## 根拠と確認できる情報

原式の新採用はない。[#719原画像調査](../handoff/Ds崩壊機構補正_Issue719_調査記録.md)の2025年第2版第1刷6.2.4イ)pp365–370、6.4.4(2)pp425–427と、#882の6.4.4(2)6)pp430–431、付録1-6.1pp793–794、ICBA Q&A No.25を引き継ぐ。原画像を今回再確認したとは数えない。

既存の `EvaluationPoint`、`ConfirmedStepResponse`、保存解析条件で、目的・生成入力・run・方向・確定step、同時点の部材応力、物理要素の網羅、層切断面と外力の釣合い、負担率を検査できる。これらの検査を公開入口で先行させ、#442の混合拒否を維持する。

採用適格性の証拠は、現在の保存形式にはない。対象条件を11件に分け、不足する情報を全件表示する。連層壁・押さえ効果・ピロティ等への該当性も確認できない場合、存在しないと推定して省略しない。非崩壊階の別途Ds応力は、目的別に一つだけ保持するrun/stepで表現できない。

このため**現時点では全モデルの採用を保留する**。保留解除の肯定例はなく、チェックボックスや選定理由の「確認済み」等で解除する機能は追加しない。別途評価を保持する新しい設計、解析・余耐力法・拘束変更や押さえ効果の近似は本件対象外である。

## 要素→試験索引

試験は `crates/sepika-app/src/app/actions/design/holding_adoption.rs` に置く。

| 観測契約 | 試験と独立した期待結果 |
| --- | --- |
| 外力分布、崩壊形、非崩壊階、壁破壊βu、脆性除外、鉛直支持、未崩壊Qu | `public_entry_and_csv_report_all_missing_adoption_conditions_for_both_rank_modes`：公開入口で各条件名と採用保留を観測。自動・手動とも全件をCSV、GUI feature時は描画文字列にも保持 |
| 壁脚拘束、直交梁の作用軸力/負担せん断、ピロティ方針/単独柱軸力/性能 | 同試験：追加4条件の不足理由を観測。適用対象なしとみなさない |
| 旧合否の失効 | 同試験：数値候補が生成した採用元・βu・壁詳細を、公開保留後に除去 |
| 入力とrun等の混合 | `public_entry_keeps_run_mixing_rejection_before_adoption_check`：run不一致理由を先行表示。その他の既存入力/方向/step/目的混合試験も候補入口へ移して維持 |
| 任意終了を崩壊とみなさない | `termination_classification_and_selection_words_do_not_prove_adoption`：全6終了理由で保留。空ヒンジと全材端降伏を与え、実際の`mechanism_at`がPartial/Overallと判定することを確認してから保留を検査。hintの変更だけを根拠にしない |
| 全層Qu≥Qunでも未崩壊耐力の採用保留 | `all_story_capacity_comparisons_passing_do_not_prove_noncollapse_adoption`：人工確定履歴の層内力・外力・許容差を同率拡大し釣合いを維持、数値候補の全層比較成功をassertした上で公開採用を保留。実耐力のValidationではない |
| 既存数値の保証 | `compute_unconfirmed_holding_capacity`へ既存試験を移し、数値期待値・モデル・試験件数を維持。公開GUIの成功表示期待は保留理由へ変更し、数値候補の独立assertを残す |

GUIの描画文字列試験は手動GUI操作と区別する。公開入口では未確認の数値候補を合否として返さず、CSVでも不足理由を列挙する。計算契約版の更新と先行#871との統合は管理担当が最終構成で行う。

## 実行結果

専用`CARGO_TARGET_DIR`を`C:/Users/hiroa/Desktop/ikkan/.worktree/.cargo-target-872`に指定した。

- `rustup update stable`：unchanged、rustc1.99.1（2026-10-13）。`cargo clippy --version`：0.1.99（58cf8f9c35）。
- `cargo test -p sepika-app --lib --locked`：428成功、失敗・無視・filterは0。新規4試験を含む。
- `cargo test -p sepika-app --features gui --lib --locked`：995成功、失敗・無視・filterは0。新規4試験とGUIの全条件理由表示を含む。
- `cargo run -p xtask -- check-docs`：成功、summary124／links666／impl refs709。
- `mdbook build`：成功。

- 変更した開発文書の相対ファイルリンク725先の実在を確認。新規「要素→試験索引」へのアンカーと記録への索引追加も確認した。

- `cargo clippy -p sepika-app --features gui --all-targets --locked -- -D warnings`：成功。
- `cargo fmt --all -- --check`、`git diff --check`：成功。

## 未検証・範囲外

必要な採用証拠の取得・保存と採用成功経路、実建物のDs/Qu・機構や鉛直支持性能、手動GUI、実験Validationは未検証。通常群判定・Ds表全数値、#489再開、新しい崩壊解析や余耐力法、法規ルート網羅は対象外。#871の部材/柱脚群条件とは理由を別管理し、最終公開入口・CSVでは両者を集約する必要がある。
