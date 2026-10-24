# 階 FL・SL — Issue #729 の実装・確認記録

作成日: 2026-10-24

対象: [#729](https://github.com/hrntsm/SEPIKA/issues/729)。製品仕様は[階の分布](../../docs/preparation/01_階の分布.md)、設計判断は[ADR 0070](../adr/0070-story-fl-sl.md)を正とする。

## 実装判断と直接確認

- FL／ΔSL を Option で保持し、SL と FL／SL 間高さを導出。Layer.height は FL 差、既存の上下床標高も FL、構造端部を独立した SL 欄にした。
- 階編集は隣接対・ID を保持。既存の逆転後並替えと最下階 FL 固定を廃止した。追加・削除に伴う配列 ID の参照更新は既存方式を維持する。
- STB は元階参照 ID を保持し差分は None。原階を高さ照合で付け替えず、現在階の編集で取込元属性を変更しない。
- 生成記録へ自動所属を保持し、原階の明示所属や利用者の Node.story と区別した。階編集では実在節点の座標を変更しない。
- 不足モデルの保存／再読込と、依存する計算入口の不足停止を分離した。階を持たない低水準の重力解析は維持する。
- 既存 fixture はモデルが意図した FL 列・差分を明示する形へ更新。階推測・基部自動追加・編集後の階並替えを前提にした期待を新契約へ改訂した。

## 検証経過

- rustup update stable: 成功、rustc 1.99.1 のまま。
- cargo check --tests -p sepika-app --features gui --locked: 成功（関連利用クレートを含む）。
- core 初回: 580 件中 576 成功。旧 fixture の不正階 ID／不足上下階、旧診断文期待の 4 件を修正。独立受入 4 件は成功。
- load 初回: 274 件中 260 成功。明示階不足・壁上端を覆う階不足・旧階 ID 再採番期待を修正。
- edit 初回: 207 件中 202 成功。旧並替え／基部固定期待、FrameSpec の未指定差分、未完成モデル検証の 5 件を修正。
- IO 初回: 173 成功、4 失敗、1 ignored。None を含む取込モデルの一般検証を、計算完成度検査から分離して修正。

最終実行結果（2026-10-24）:

- 横断 `cargo test` の `--lib --features sepika-app/gui --locked --no-fail-fast`: core 581、edit 208、IO 177（ignored 1）、job 84、load 276、solver 319 件成功。App／design は旧 fixture の不足階・不在 ID 期待を是正して個別再実行した。
- PR 最終監査で、GUI に限定していた StoryId import を default でも使うテスト helper が参照する不備を修正。`cargo check -p sepika-app --tests --locked`（GUI なし）成功、追加のコンパイル不備なし。ログ `sepika729-app-default-check.log`。
- `cargo test -p sepika-design-jp --lib --locked`: 645 件成功。
- `cargo test -p sepika-app -p sepika-mcp --features sepika-app/gui --lib --locked --no-fail-fast`: App は 899 件成功。MCP の不足階／原階参照の fixture 2 件を是正し、mcp feature を含めて再実行した。
- `cargo test -p sepika-mcp --features mcp --lib --locked`: 共通 pushover fixture の矛盾した所属 ID も修正し、131 件成功。
- `cargo test -p sepika-app --features gui --test full_model --test wall_model --test source_story --locked --no-fail-fast`: 37／16／13 件、計 66 件成功。
- `cargo test -p sepika-job --test wall_weight_partition -p sepika-solver --test reference_boundaries --locked --no-fail-fast`: 23／1 件成功。
- 追加利用側 integration: core/reference_boundaries 2、IO/ovika_reference_boundaries 1、design/vnv_rigid_zone_ds 3、job/high_density_steel 3・seismic_freshness 9・steel_slab_density 6、solver/panel_zone 18・reference_boundaries 1、計 43 件成功。
- `cargo test -p sepika-job --test damper_mass_errors -p sepika-solver --test weight_mass --locked --no-fail-fast`: 6／12 件成功。

- 関連 9 クレートの `cargo clippy --all-targets --features sepika-app/gui,sepika-mcp/mcp --locked -- -D warnings`: 成功。
- `cargo fmt --all --check`、`cargo run -p xtask --locked -- check-docs`、`mdbook build`: 成功。

横断の初回失敗を成功とは扱わず、fixture 修正後の該当クレート・対象テストの再実行を最終結果とする。ログは作業環境の TEMP ディレクトリの `sepika729-*.log` に保持した。
旧 main 比較の後に共有 target の古い型成果物を再使用して起きた App コンパイルエラーは、現ソースを再ビルドした App＋MCP 構成の全テスト成功で解消を確認した。

## 直接受入テスト

- core model::story::level_tests: FL 3500／7000、差分 −500／−600、SL 3000／6400、FL 高 3500／SL 高 3400。差分のみ／FL の変更、負0正差分、未設定・非有限・導出 overflow・重複逆転・参照切れ、フレームの差分不足。
- load story_gen の SL 受入: SL 面判定、明示所属と利用者剛床保持、面外中間節点の除外、最下 FL と構造最下端の分離、元節点不動。
- App fl_sl_*: 共通準備、STB の不足入力と元属性保持、保存再読込、GUI 保留編集・Undo/Redo と結果入力識別の失効。

## 対象外と確認限界

個別梁／床板偏心は #726／#727、解析剛性へ偏心を反映する処理は #734、SL 節点変位差と FL 高を使う層間変形角の具体的算定・集計・表示は #490 が担当する。
今回の独立算術期待値は階情報と供給先の確認であり、利用側の算定修正を確認済みとはしない。

RC の固定柱 1 本＋片持ち梁 1 本の L 形架構で、自動剛床を生成すると特異行列となる独立問題候補は [#786](https://github.com/hrntsm/SEPIKA/issues/786) に記録した。旧 main `94f379e7` の同じ RC 試験へ Story.elevation=0／3000 の入力だけを追加し、元の支持・明示拘束なしの条件で比較した。
`cargo test --manifest-path .tmp-729-baseline/Cargo.toml --target-dir target -p sepika-app --features gui --lib --locked app::tests::test_compute_ultimate_checks_rc_frame -- --exact --nocapture` は、現在ブランチの FL=0／3000・ΔSL=0 と同じ「剛性行列が特異・3 節点に拘束」で失敗した（ログ `sepika729-baseline-rc.log`）。この同条件比較により本件の退行と区別した。RC 終局検定の fixture は上端 2 節点の Rx／Rz 回転支持と実節点を代表にした明示剛床を指定し（水平並進は自由）、今回の変更へ解析方式の修正は含めない。

## 独立レビューで是正した事項

- 壁重量の保存階帯照合は、未設定階を filter_map で除去せず、完全性検査後の同長 SL 列を使う。全／中間差分欠落と中間 FL 欠落を回帰対象にした。
- フレーム入力は正の入力階高だけでなく、浮動小数丸め後の導出 FL 差も有限正値であることを要求する。1e20 + 1 の重複を検証する。
- 柱抽出で未完成の上下階を構造最下端へ置換する fallback を廃止した。
- 原階所属解除後に取込時 Node.story が明示所属として復活しないよう、原階参照を持つ階では原階一覧を正本とした。解除・SL 編集・再準備・正順 Undo/Redo と旧階重量消失を検証する。
- 質点表示の構造座標は SL から取り、計算用 FL 間高さを積み上げない。平面位置整列の有無とも同じ SL を使う。
- 階構造入力の前検査を共用し、不成立は停止する。階以外の既存重量生成エラーが独立重力解析まで一律停止する変更は取り消した。


## CI 追加監査と App integration の統合確認

CI が検出した `story_application_lifecycle` の上階削除後の再生成期待は、階が 1 個だけでも生成できる旧契約に依存していた。
再生成と正順 Undo/Redo の完全復元は、明示 FL=0／1750／3500 mm・ΔSL=0 の 3 階から中間階を削除し、有効な 2 階を残す fixture で維持した。
最上階の削除で上下階対を失う場合は、生成停止、FL／ΔSL と節点座標の保持、旧重量生成記録の失効、失効と階削除を正順に Undo して完全復元することを独立して確認した。

同時に App integration 全 14 バイナリを検索・確認した。`wall_post_model` の既知 1 層（0／3000 mm）と手動 `perf_probe` の指定階数・3500 mm 階高も、fixture 側で FL 列と ΔSL=0 を明示した。製品本体の推測・補完は追加していない。

- `cargo test -p sepika-app --features gui --test story_application_lifecycle --test source_story --test wall_post_model --test lip_channel --test full_model --test slab_physical_mass --test rc_section_purpose --test wall_model --locked --no-fail-fast`: 93 件成功。
- `cargo test -p sepika-app --features gui --test attached_slab_import --test beam_contact_quantity --test circular_post_quantity --test load_state_report --test wall_check_status --test perf_probe --locked --no-fail-fast`: 10 件成功、手動性能診断 1 件 ignored（コンパイル確認のみ）。

全体では 103 件成功・0 件失敗・1 件 ignored。ログは `sepika729-app-lifecycle-related.log` と `sepika729-app-other-integration.log`。
共有 target に残っていた旧 main の型成果物によるローカル初回コンパイル失敗は、関連 package 成果物の clean と現ソースの再ビルド後に解消した。

- `cargo test -p sepika-app --test story_application_lifecycle --locked`: default 構成でも 3 件成功（`sepika729-lifecycle-default.log`）。
- `cargo clippy -p sepika-app --features gui --test story_application_lifecycle --test perf_probe --locked --message-format short -- -D warnings` と、同設定の `--test wall_post_model`: 成功。
- `cargo fmt --all --check`、`git diff --check`: 成功。


## default workspace 全体の追加確認

CI の逐次停止による後続 integration の確認漏れを収束させるため、`cargo test --workspace --locked --no-fail-fast` をローカルで全実行した。
初回は unit／integration／doc tests の計 101 群を完走し、3894 件成功・2 件失敗・4 件 ignored。失敗は次の明示階不足 fixture だけだった。

- `sepika-load/tests/generated_master_references.rs`: 非アクティブ生成代表節点の再利用と入力不変性を確かめる 0／3000 mm モデルに、FL／ΔSL 入力がなかった。
- `sepika-solver/tests/common_section_mass.rs`: SRC の物理重量と設計 DL の使い分けを確かめる生成経路に、0／3000 mm の明示階入力がなかった。

両 fixture に FL=0／3000 mm・ΔSL=0 を明示した。再利用・不変性・独立重量期待値の assert と製品本体は変更していない。
再実行した同じ workspace コマンドは exit 0、101 群・3896 件成功・0 件失敗・4 件 ignored。両対象も成功し、`sepika-job/tests/slab_physical_mass.rs` を含む後続群と doc tests まで確認した。
ログは TEMP の `sepika729-workspace-default.log` と `sepika729-workspace-default2.log`。

`cargo clippy -p sepika-load --test generated_master_references -p sepika-solver --test common_section_mass --locked --message-format short -- -D warnings` は成功。
`cargo fmt --all --check` と `git diff --check` も成功。GUI／MCP／Parquet 構成に影響する製品ソース変更はなく、既成功の feature 検証は無条件に繰り返していない。


## 未設定の直上階を飛ばす壁高さ導出の是正

独立レビューで、自立壁の階高解決が `filter_map` で未設定階を取り除き、さらに上の SL を直上階として使う経路を確認した。
`try_story_height_above`／`try_wall_plate_extent` は階順に必要な FL・ΔSL を解決し、不足を階 ID・項目名付きで返す。既存 Option API は解決失敗を None とする。
数量入口は高さが階に依存する壁だけ診断付きの解決を要求し、未解決の高さからコンクリート・型枠の正常数量を返さない。明示寸法・階高不要の数量と入力モデルは維持する。
直上 SL が確定した後の未使用階の欠落は、その壁高さには必要としない。

横断確認では、計算で階値を除外する別の `filter_map` は地震用建物高さにあったが、走査する `Model::layers` は完全性検査済みであり、未設定階を圧縮する経路ではない。
壁重量の階帯は完全性検査後の同長 SL 列を使う。外周スラブの SL 判定は同一面の一致判定で、未設定階から別の高さを借用しない。
ネイティブ原階の FL フィルターは未解決属性を原階へ複写しない保存上の扱いで、計算用高さの補完ではない。グリッド・構造ビューは未解決 SL を描かず、入力一覧は未設定を示す。

- core 回帰: 中間階 ΔSL=None／FL=None、階 ID・不足項目診断、境界・自重未解決、入力不変、明示寸法保持、完成後の直上高さ 3000 mm、さらに上の未使用階の欠落。
- quantity 回帰: 同じ中間階欠落を壁 ID 付き Err とし、明示高さ 1000 mm は 0.9 m³／12 m²、完成した自動高さは 2.7 m³／36 m²。階不要の部材数量は継続できる。
- `cargo test -p sepika-core -p sepika-design-jp -p sepika-load -p sepika-job --lib --locked --no-fail-fast`: 582／646／276／84 件成功。
- `cargo test -p sepika-app --features gui --test beam_contact_quantity --test circular_post_quantity --test wall_post_model --test wall_model --locked --no-fail-fast`: 29 件成功。
- 関連 4 クレート `cargo clippy --all-targets --locked -- -D warnings`: 成功。fmt／diff check、check-docs、mdbook build 成功。

ログは TEMP の `sepika729-wall-height-*.log`。
