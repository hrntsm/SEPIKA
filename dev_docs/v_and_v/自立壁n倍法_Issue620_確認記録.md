# 自立壁n倍法の点検（Issue #620）

2026-10-25、main `49e83a70` 由来の作業ブランチを点検した。現在仕様の正本は[形状係数Fes](../../docs/calc_basis/07_二次設計/04_形状係数_Fes.md)であり、本書は確認経路と有限例のVerificationを記録する。

## 到達性と修正の対応

| 点検対象 | 既存の入口・上流検査 | 判定と対応 |
| --- | --- | --- |
| 係数n | `StressCfg.misc_wall_n` とappend。正値・有限性の拒否はなかった | 考慮指定時に拒否。壁なしでも不正nを許容しない |
| 無開口RC | `self_standing_stiffness` は床アンカーと厚さ・高さだけを判定 | 非RC・有開口・スリット混入は成立。既知の適用外を除外し、材料不明を診断 |
| 参照不能 | `Model::validate_reference_ids` は節点・断面・材料のダングリングを拒否 | 公開解析でのダングリング混入仮説は不成立。n倍法の直接入口では不足を対象付きで返す |
| 未割当材料 | 略算 `column_stiffnesses` はskip、面積は独立集計。`validate_reference_ids` はNoneを許容するが、静的 `model_issues` は材料未割当を拒否 | 正常なGUI新規静的結果へ混入する仮説は不成立。直接略算/appendでは再現できるため、n倍法の必須入力契約として全柱の局所検査を行い、除外しない |
| 分割柱 | `story_columns` が最上部材を代表にしていた | 既存トレースで連なりの全部材IDを保持。n倍法だけで断面割当の同一性を検査。柱どうしの断面差は許容 |
| 応答欠落の0化 | `column_stiffnesses_from_analysis` 単体は不足時0、製品の `story_eccentricity_from_analysis` は前段でX/Y変位・代表部材内力の欠損/非有限を拒否 | Re製品入口での仮説は不成立。一方、既存3D質点系は下位を直接呼ぶため不足の0化が到達する。上位の既存検査を下位へ移して共用し、内力取得を微小変位の判定より先にする |
| 階の有限性 | `Story::sl_mm` はFL・ΔSL・和の非有限をNone、`try_story_height_above` は階入力診断と全階FL/ΔSLを検査 | NaNが帰属判定falseになり黙って消える仮説は不成立。既存の取得失敗を伝播 |
| 符号付き壁高さ | 壁版の既存契約は負方向の立ち上がりを許容（`attached_area_uses_abs_height_when_extent_sign_reverses`） | 有限な負高さは不正値として新規禁止せず、直上階に未到達という既存判定で除外。非有限は診断 |
| 有限性・符号・積overflow | n倍法の防御は柱総面積<=0のみ | n/壁面積/柱面積は正有限、柱剛性は非負有限、計算結果は有限を要求。失敗時は追加前に停止 |

## 柱集合・方向・入力世代

略算と精算の剛性列挙は `story_columns` の順序を使う。略算のskip条件のうち、材料は面積集計時に全連なりで拒否する。層上下端へ到達する柱の高さは層境界の既存検査で正となり、無効局所軸は略算自身がErrを返す。型・接続節点は柱列挙で検査する。n倍法の面積も同じ `story_columns` で同じ連なりを評価し、全列挙柱の必須入力を検査する。件数一致だけで集合一致を保証せず、略算の材料等によるskipを入力検査で許さないことと、列挙実装・順序の共有を併せて確認する。剛性列との件数差・不正な方向別値を拒否する。各柱を面積で一度ずつ数え、壁を追加する前のX/Y剛性だけを集計する。

GUI/CSVは `summary::seismic_story_metrics` がX/Y地震静的と採用長期組合せについて `require_result_input` を通し、失敗を `StoryMetricsCtx.re_error` からRe/Fesへ伝播する。保有耐力は `actions/design/holding.rs` の依存成果照合を通して `compute_story_metrics_with` を使う。直接の純計算関数へ新しい世代管理は追加しない。関連する入力世代試験は[層指標の生成入力照合](層指標の生成入力照合_Issue810_2026-10.md)を再利用する。

保存結果の検定適用条件が変わるため `DESIGN_CONTRACT_VERSION` を28から29へ更新する。既存3D質点系の生成条件も変わるため、解析契約は最新main `d00b3c29`（Issue #618、解析契約53）の統合後に54へ更新する。初回実装の解析契約53をmainの53と同一契約として再利用しない。

## 独立期待値と試験

`eccentricity/tests.rs` の `misc620_*` と既存 `test_append_misc_wall_stiffnesses`、`test_column_stiffnesses_merges_split_column` を使用する。

- n=1、Aw=200000 mm²、ΣAc=160000 mm²、ΣKc=10000 N/mmから12500 N/mm。
- ΣKc=100+400、ΣAc=100+200から5/3。指定柱方式へ切り替えない。
- 45°壁にX/Y独立の集計を与え6250/12500 N/mm。方向余弦投影の有限照合であり、一般XY連成省略の妥当性確認ではない。
- 材料を取得できない別柱では `model_issues` が上流で材料未割当を拒否することと、直接略算ではskipされることを別々にassertし、直接n倍法入口でも停止することを確認する。柱面積の負値なども上流solver検査とn倍法契約を混同しない。
- 柱面積0/負/NaN/Inf、係数0/負/NaN/Inf、剛性負/非有限/合計overflow、壁面積overflowを拒否。
- RCと同幾何の非RC、開口、スリット、材料欠落を区別。同断面分割は集計、層内異断面は階・柱・理由付きで停止。n未指定/対象壁なしなら異断面制限は発動しない。
- `eccentricity_analysis::misc620_tests` は応答が存在するせん断力0と、部材力欠落によるErrを区別。ゼロ剛性から有限Reが常に得られるという主張はしない。
- 最上階の明示高さパラペットは既知適用外として除外する。階高いっぱい指定で直上階が未定義の場合や、階入力自体が不足する場合は診断を返す。
- 階SL=[0,3000,6000,9000] mm、壁6500 mmを直上1層だけへ算入。上階SL編集、支持床変更、壁削除で再評価。既存n未指定試験も維持。

## 直接3D・2D/3D質点系への影響

変更箇所は偏心率の付加剛性とその失敗伝播である。直接3Dの静的・時刻歴solverへ自立壁要素を追加しない。`sepika-job/src/lumped_mass.rs` の既存 `build_spatial` は `column_stiffnesses_from_analysis` とappendを直接使用するため、今回の適用条件検査と失敗を `InvalidInput` として伝播する。既存ColumnKi方式では付加剛性が方向別並進剛性へ、両方式で剛心とねじり剛性へ影響する。層Q/δ方式では付加後の剛心位置による変位へ影響する。`build_planar` はappendを呼ばず、既存2Dの剛性生成へ新規接続しない。Rsも変更しない。

`lumped_mass::tests::misc620_spatial_propagates_invalid_n_and_missing_column_response` と既存lumped_mass試験で既存生成経路を検証する。`misc620_existing_spatial_column_ki_includes_wall_but_planar_does_not` はAw=200000 mm²、ΣAc=160000 mm²、ΣKx=10000 N/mmで既存3Dの層並進剛性22500 N/mm、2Dは10000 N/mm、3D剛心Y=5000/9 mm、上層へ再算入されないことを独立照合する。2D/3D質点系と直接3D時刻歴の自立壁専用動的比較、手動GUI、実験・実測Validationは未実施。Re/Fes算定不能の既存利用側試験と区別する。

## 公開元の確認範囲

管理担当が2026-10-25に公開元を実取得して確認した記録を共有した。資料は採用例であり法令上唯一の方法とは扱わない。追加除外規則や指定柱方式は採用しない。

- [公開計算資料 §7](https://kke-resp-team.github.io/resp-d-docs/calculation2/03_stress_analysis/03_eccentricity_ratio/index.html): 層内合算式。
- [公開Q&A BUS00620](https://support.kozo.co.jp/support/disp.php?p1=1539&q2=bus): 層・方向別の合算と柱総面積0の診断。1539はURL識別であり文書番号ではない。
- [公開Q&A NBUS00064](https://support.kozo.co.jp/support/disp.php?p1=3736): RC対象・柱選択等の運用条件。そこでのRC/CFT限定やその他の除外条件を本実装へ横断適用していない。

2020年版技術基準解説書付録1-3.1 (9)⑤は原書未閲覧。2020/2025年版適合確認済みとはしない。Rs接続調査は[Issue #740](https://github.com/hrntsm/SEPIKA/issues/740)へ分離したままである。

## ローカル検証

全Cargoコマンドは `CARGO_TARGET_DIR=C:\Users\hiroa\Desktop\ikkan\target` で実行した。

- `rustup update stable`: 成功、rustc 1.99.1、clippy 0.1.99（2026-10-13）。
- `cargo test -p sepika-design-jp --locked`: 単体665件・統合13件成功。下位応答検査移動後にも全件成功。
- `cargo test -p sepika-core --locked`: 単体591件と各統合試験成功、perf probe 1件は既定ignored。
- `cargo test -p sepika-job --lib lumped_mass --locked`: 18件成功。初回は新規壁fixtureの重量生成方式不足と、既存非有限試験の診断期待（検査を前段化したため）の2件が失敗し、fixture・期待を修正後に成功。
- `cargo test -p sepika-app --lib summary --locked`: 17件成功。#810の入力世代・方向・長期結果gateとGUI/CSV共通計算入口を含む。
- `cargo clippy -p sepika-core -p sepika-design-jp -p sepika-job -p sepika-app --all-targets --locked -- -D warnings`: 成功。
- `cargo run -p xtask -- check-docs`: 成功（summary 124、links 640、impl refs 684）。
- `mdbook build`: 成功。`cargo fmt --all --check` と `git diff --check` も成功。

初回の略算の材料欠落別柱試験は、別柱へ欠落断面を割り当てていないfixture誤りで失敗し、入力を訂正して再試験した。これらの失敗を製品計算の不具合の証拠とは扱わない。CI・独立レビュー・mergeは本実行記録とは別に確認する。


## 最新mainとの統合

2026-10-25に `origin/main` の `d00b3c29` を取得し、`9210de26` で統合した。コードの競合はなく、検証目録と未検証一覧の追加項目だけを両方保持して解消した。
Issue #618の左右床板平均厚、梁偏心を構造せいの半分下とする実モデルfixture、断面用途の検査はそのまま保持する。n倍法の式・対象や柱断面にその平均厚を転用しない。

解析契約54・検定契約29の統合状態で以下を再実行し、すべて成功した。Cargoの対象ディレクトリは上記と同じ。

- `rustup update stable`（1.99.1）、`cargo fmt --all --check`、`git diff --check`。
- `cargo test -p sepika-design-jp --locked`: 単体665件・統合13件。
- `cargo test -p sepika-job --lib lumped_mass --locked`: 18件。
- `cargo test -p sepika-app --lib summary --locked`: 17件。
- `cargo test -p sepika-app --test full_model story_metrics_computed_for_every_layer --locked`: 1件。#618の明示配置fixtureで全層のRs/Re/Fes算定を確認した。
- `cargo clippy -p sepika-core -p sepika-design-jp -p sepika-job -p sepika-app --all-targets --locked -- -D warnings`。
- `cargo run -p xtask -- check-docs`: summary 124、links 642、impl refs 685。
- `mdbook build`。

main統合による計算コードの手動修正は行っていない。独立fresh reviewと最終headのCIは管理側で確認する。
