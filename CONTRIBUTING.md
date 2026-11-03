# コントリビューションガイド

本書は開発参加の入口です。詳細な正本は [文書配置の索引](dev_docs/README.md) から確認してください。

## 開発参加時の基本事項

- コミットメッセージ・コード中のコメント・Issue / PR のやりとりは日本語で行ってください。
- [Rust ツールチェイン](https://rustup.rs/) の最新 stable を使用してください。静的解析の前に `rustup update stable` で更新し、`cargo clippy --version` で確認します。`--locked` は依存解決を固定するもので、ツールチェインは固定しません。
- 実装・コード・テスト・コメントの原則は [AGENTS.md](AGENTS.md)、文書の配置は [dev_docs/README.md](dev_docs/README.md) を参照してください。

## Git コミットルール

コミットメッセージの 1 行目と PR タイトルは、`<プレフィックス>: <簡潔な変更内容>` とします。

| プレフィックス | 変更内容 |
| --- | --- |
| `feat` | 新しい機能 |
| `fix` | バグの修正 |
| `docs` | ドキュメントのみの変更 |
| `style` | 空白・フォーマットなど、動作に影響しない変更 |
| `refactor` | 仕様に影響しないコード改善 |
| `perf` | パフォーマンス向上 |
| `test` | テスト関連 |
| `chore` | ビルド・補助ツール・ライブラリ関連 |

コミットの理由・意図の記録は [AGENTS.md](AGENTS.md#how--what--why--why-not) に従ってください。

- 論理的な区切りごとに、1 コミットへ 1 つの目的をまとめます。
- コードの変更には対応するテストを含め、ドキュメントのみの更新は別コミットに分けます。
- コミット前に下記の原則で必要な検証を行い、区切りで `git status` を確認します。

## ローカル検証の原則

開発中は変更したクレートと、作業に直接関係するテスト・静的解析を中心に実行します。

```bash
cargo test -p <changed-crate>
cargo clippy -p <changed-crate> --all-targets --locked -- -D warnings
```

非デフォルト feature 配下を変更した場合は、対象クレートが持つ関連 feature を有効にして確認します。
feature 定義は各クレートの `Cargo.toml` を参照してください。

PR 前は、変更範囲と合理的に影響を受ける範囲を検証します。共通型・データモデル・解析コア・
シリアライズやファイル形式・workspace や Cargo feature・クレート間依存・複数クレートの API
を変更した場合は、利用側や関連構成まで範囲を広げます。
無関係なクレートや統合テストを、PR 前という理由だけで毎回ローカル実行することは必須ではありません。

新しいクレート間依存を追加した場合は `cargo run -p xtask -- check-deps` を実行し、
依存方向を確認してください。文書変更時の検証は [文書の執筆・検証](dev_docs/docs_authoring.md) を参照してください。

リポジトリ全体の網羅的な回帰検証は CI が担当します。実際のコマンド・構成の正本は
[CI workflow](.github/workflows/ci.yml) とし、本書へ一覧を複製しません。

`main` への push（PR の merge）ではテスト用キャッシュ生成ジョブのみを実行し、新規 PR が
初回から復元できるテスト用依存ビルドキャッシュ（rust-cache）を生成・更新します。
キャッシュキーが変わった時だけ再ビルドし、品質ゲート（テスト実行・静的解析・ビルド確認）は
PR でのみ実施します。PR ではテスト構成ごとにジョブを分けて並列実行し、全構成が成功した
場合のみ必須チェック「テスト」が成功します。

Windows固有の名前付きパイプ・GUI検討の実通信・MCP stdio中継は、専用のWindowsジョブで検証し、
必須チェック「テスト」へ含めます。mainでは同じWindows構成を`--no-run`でビルドして専用キャッシュを
生成し、完全一致のキャッシュがある場合は再ビルドしません。PRはそのキャッシュを復元し、更新しません。

CI ではビルド成果物の容量を抑えるため、dev / test profile のデバッグ情報を生成しません。
デバッグアサーション・整数オーバーフロー検査と検証対象は維持します。
Linux のテスト用キャッシュ生成・各テスト構成・Clippy は、ビルド前後の空き容量と `target` の
使用量をログへ記録します。`rust-cache` はこれらの profile 環境変数を復元キーへ含めるため、
デバッグ情報を含む旧 Linux キャッシュは復元しません。

## テスト入力と補助コードの配置

- `src/` 内の単体テストは、対象モジュールの契約・内部状態を検証する。GUI操作・描画に依存する補助だけをGUI内部に置く。
- `tests/*.rs` はクレートの公開入口・複数機能を接続する統合テストを置く。テスト本体を別ファイルから読み込む場合も、親の統合テストを実行入口とする。
- `tests/support/` はモデル構築・操作・通信などのRust/Python補助コードを置く。固定入力ファイルは `tests/fixtures/` に置き、読み込み側で期待結果を検証する。
- 新しいフォルダを増やさず既存の検証責務を維持するため、`sepika-job/tests/fixtures/` と `sepika-design-jp/tests/fixtures/` のRust補助コードは既存位置で管理する。モデル生成は `*_model.rs`、独立期待値とassertは `*_reference.rs` とし、読み込むSTB・JSON・TSV等と区別する。
- 管理先は検証対象を所有するクレートで決める。core型だけを使う入力でも、解析・耐力・数量などの仕様を検証するものはその責務側に置く。共有のための製品API・依存・共有クレートは追加しない。
- 共有入力は一か所で構築し、各利用側のテスト・独立期待値・許容誤差を保持する。共通の期待契約を使う場合も、入力構築とは別ファイルにする。

### クレートをまたぐ共有の管理先

以下のパスは `crates/` からの相対パス。利用元は直接のソース参照に加え、親モジュール経由の入口も示す。

| 管理先 | 共有する検証対象・管理理由 | 利用元・実行入口 |
| --- | --- | --- |
| `sepika-core/tests/support/assignment_identity_model.rs` | 割当領域の識別・外部ID。coreのモデル契約を各入口でも維持 | core `model/assignment_identity_tests`、edit/job/mcp `tests/assignment_identity.rs`、mcp `tests/external_ids.rs`・`src/server_external_ids.rs`、app `src/app/assignment_identity_tests.rs` |
| `sepika-core/tests/support/frame_support_model.rs` | 主架構支持の解析表現。coreの物理支持契約を編集・入出力・準備で維持 | core/edit/io/job `tests/frame_support_representation.rs` |
| `sepika-core/tests/support/secondary_materialization_model.rs` | 二次部材の解析要素化。coreの生成契約をジョブ・GUI入口で維持 | core/job `tests/secondary_materialization.rs`、app `src/app/tests.rs` |
| `sepika-core/tests/support/steel_secondary_post_model.rs` | 鋼製間柱のモデル属性・材料対応。編集・検定・GUIで同じ入力を保持 | edit `src/secondary.rs`、design-jp `src/post.rs`、app `src/app/post_design_tests.rs` |
| `sepika-solver/tests/support/static_analysis_model.rs` | 柱の軸剛性設定と引張専用ブレース反復。solverの静的解析契約を各実行入口で維持 | solver `tests/parallel_batch.rs`、job `src/compute.rs`、app `src/app/actions/linear_static.rs`、mcp `src/tests.rs` |
| `sepika-solver/tests/support/rc_wall_side_column_model.rs` | 側柱付きRC壁の共通face・剛性・力の分担。solverの壁系解析を要素・検定・重量・GUI/MCPでも検証 | solver `tests/wall_system_linear.rs`・`src/nonlinear/wall_reference.rs`、element `src/wall/static_face.rs`、design-jp `src/joint_wiring/tests.rs`、job `tests/wall_weight_partition.rs`、app `src/app/rc_wall_side_column_tests.rs`（holding・viewerからも利用）、mcp `src/job/mod.rs`経由のdesign_check・pushover・time_history・server |
| `sepika-solver/tests/support/wall_side_column.rs` | 共通断面の質量・慣性。solverの解析質量を準備・GUI入口でも検証 | solver `tests/common_section_mass.rs`、job `tests/wall_side_column_entry.rs`、app `src/app/tests.rs` |
| `sepika-design-jp/tests/fixtures/rc_wall_cracking_model.rs`・`rc_wall_cracking_reference.rs` | RC壁のひび割れ耐力の入力と独立期待契約。design-jpの耐力計算をGUI・MCPでも照合 | design-jp `src/joint_wiring/tests.rs`、app `src/app/rc_wall_cracking_tests.rs`、mcp `src/job/design_check.rs`・`src/server.rs`（`src/job/mod.rs`から共有） |
| `sepika-design-jp/tests/fixtures/signed_beam_gravity_model.rs` | RC梁の符号付き重力端反力と検定組合せ。design-jpのせん断需要算定に共通の入力 | app `src/app/signed_q0_acceptance_tests.rs`（viewerからも利用）、mcp `src/job/signed_q0_acceptance_tests.rs` |
| `sepika-design-jp/tests/fixtures/nonnodal_quantity_model.rs`・`nonnodal_quantity_reference.rs` | 非節点小梁の材料別数量。design-jpの数量計算とassertをGUI・MCPの編集操作から検証 | app `src/app/nonnodal_quantity_acceptance_tests.rs`、mcp `src/nonnodal_quantity_acceptance_tests.rs` |
| `sepika-design-jp/tests/fixtures/lateral_bracing_model.rs` | 横補剛の検定入力。GUI・MCPからdesign-jpの同一検定契約を検証 | app `src/app/tests.rs`、mcp `src/tests.rs` |
| `sepika-job/tests/fixtures/steel_slab_model.rs` | 鋼床板の密度・物理質量。jobの重量準備をGUI・MCP入口でも検証 | job `tests/steel_slab_density.rs`・`tests/slab_physical_mass.rs`、app `tests/slab_physical_mass.rs`・`src/app/tests.rs`、mcp `src/server.rs` |
| `sepika-job/tests/fixtures/seismic_freshness_model.rs`・`high_density_steel_model.rs` | 地震荷重の更新と高密度鋼材。jobの荷重準備・検証契約をGUI・MCPからも検証。鋼床板モデルもこの入力を使用 | job `tests/seismic_freshness.rs`・`tests/high_density_steel.rs`・`tests/member_load_direction.rs`、app `src/app/tests.rs`、mcp `src/server.rs` |

appの `tests/support/frame_gravity.rs` は `full_model.rs`・`wall_model.rs` の保存重力組合せ選択、
`full_model_study.rs`・`full_model_study_wire.rs` は `full_model.rs` のGUI検討・Windows実通信テストを管理する。
mcpの `tests/support/gui_probe.py` はGUI中継の外部プロセス操作を管理する。
これらはGUI・通信固有なので計算用の共有入力へ移さない。

### 参照元と検証の追跡

`#[path]` と `include!(concat!(env!("CARGO_MANIFEST_DIR"), ...))` によるソース共有はCargo依存だけでは追跡できない。
変更ファイル名を `rg 'static_analysis_model.rs' crates` のように検索し、親モジュールから使う入口も上表で確認する。
共有先を移動・追加したら上表を更新する。入力変更時には利用元すべての単体・統合テストを実行し、
appの `gui`、mcpの `mcp` など利用元のfeature経路も確認する。具体的なコマンド構成は[CI workflow](.github/workflows/ci.yml)を正とする。
過去のV&V文書・保存ログ内のパス・コマンド・ハッシュは記録時点の証跡であり、現行の入口とは区別する。

## Issue の起票

GitHub の Issue 作成画面で、目的に合う [Issue Forms](.github/ISSUE_TEMPLATE/) を選んでください。
入力項目は各フォームを正とし、詳細な実装方法は調査・設計・実装時に決定します。
分類しづらい親 Issue や初期調査には blank issue も利用できます。

### Issue の書き方

Issue は、**構造設計・解析や機能の利用者が、タイトルと冒頭だけで課題を理解できる文章**にしてください。実装の詳細を知らなければ意味が分からない説明は避けます。

- **タイトル・背景・目的には一般的な用語を使う。** 建築構造の用語や、GUIに表示される機能名を優先します。独自の造語・曖昧な略語・内部型名を説明なく用いないでください。例：×「非所有Beamの物理断面力を是正」→○「集中ばね梁の部材荷重を断面力に反映」。
- **新しい用語が必要なら定義する。** 既存の用語では表せない場合だけ使い、初出で意味を説明します。ドメイン概念として定着させる場合は [CONTEXT.md](CONTEXT.md) と整合させてください。
- **現象と期待結果を先に書く。** 不具合なら「どの条件で何が起きるか」「本来どうなるべきか」を示します。計算結果には可能な範囲で数値・単位・独立した根拠を添えます。調査Issueなら、何が未解明で何を判断したいかを示します。
- **確認済みと未確認を区別する。** 実行結果、ソースを読んだ推定、未検証の影響を混同しないでください。分からない事項を断定せず、必要なら「未確認」と記載します。
- **実装用語は後段に分ける。** `Ku`、`state_member_forces`、所有／非所有などの内部用語、コード上の型・関数・経路は、必要なときだけ「実装調査の手掛かり」や「関連情報」に記載します。課題そのものの名称として使わないでください。
- **完了条件は外部から確認できる結果・仕様で示す。** 調査前から過度に実装方式や作業範囲を固定しません。

### Issue の作業単位

**1 Issue＝独立して完了を判断できる成果1つ**を基本とします。タイトルと目的でその成果を明示してください。
完了条件のチェック項目は複数でも構いませんが、同じ成果を確認する条件に絞ります。

- その成果に必要な実装・GUI接続・テスト・文書更新は、同じIssueに含めます。変更ファイルや分野の数、ラベルを1つにする都合で分割しません。
- 一方を未着手のままでも他方を完了できる、独立した目的が混在するときに分割します。直接必要でない機能追加や全面的な作り直しを混ぜません。
- 採用する仕様や実施の要否を判断すること自体が成果となる場合は、調査を別Issueにします。目的と完了成果が確定している作業の原因調査・実装方式の検討・通常の検証は、同じIssueに含めます。
- 親Issueは目的・子Issue・依存関係・全体の完了条件をまとめる集約用とし、実作業は子Issueで管理します。親であることを作業種類にはしません。

### Issue の種類・分野

Issue の分類は **GitHub ラベルを正本** とします。タイトルには `fix:`・`feat:`・`investigate:` 等の
種類プレフィックスを付けず、目的が分かる簡潔な日本語にしてください。
このルールは Issue に限り、コミットメッセージと PR タイトルには上記のプレフィックス規則を引き続き適用します。

- **種類ラベルは原則1つ**：完了成果の主目的から選びます。実装に伴うテスト・文書更新などの工程ごとには付けません。
- **分野ラベルは原則1つ**：変更ファイルの所在ではなく、成果の主な対象から選びます。横断的な作業で主分野を定められない場合は無理に付けず、対象範囲を本文で説明します。
- 色分けは種類を濃色、分野を淡色にすることを推奨します。分類はラベル名と定義で判断し、作業状態・優先度は別の観点として扱います。
- 不具合修正フォームと調査・検証フォームは対応する種類ラベルを自動付与します。機能追加・変更・整備フォームとblank issueでは種類を手動設定し、分野はいずれのフォームでも手動設定してください。

#### 作業の種類

| ラベル | 主目的 |
| --- | --- |
| `機能追加・変更` | 対応範囲・操作・計算方式などの仕様を追加・変更する。利用者に影響する機能削除も含む。 |
| `不具合修正` | 本来の仕様・期待する結果と異なる動作を直す。 |
| `ドキュメント整備` | 文書の誤り・不足・構成・用語・リンクなどを整えることを成果とする。 |
| `リファクタリング` | 計算結果・利用者向け仕様を維持し、コードやテストの構成・重複・不要処理を整理する。 |
| `性能改善` | 計算・描画・起動・ビルド・テストの時間や資源使用量を改善する。 |
| `調査・検証` | 原典照合・方式比較・独立検証で結論と根拠を得る、または試験を追加して仕様を継続的に確認できるようにする。 |
| `保守更新` | 依存関係・ツール・設定・配布手順などを更新して開発・提供を維持する。不具合修正や性能改善などが主目的なら、その種類を選ぶ。 |

#### 対象分野

製品に関する分野は[利用マニュアルの章構成](docs/SUMMARY.md)を参考にした分類です。
UIの修正も操作・入力・表示の対象で分類し、ファイル形式や構造種別ごとのラベルは増やしません。
既存の分類では扱いにくい独立した対象が繰り返し現れた場合は、分野の追加・分割を検討します。

| ラベル | 主な対象 |
| --- | --- |
| `基本操作・モデル入力` | 基本操作、架構・部材・属性などの入力と編集。 |
| `解析実行・結果表示` | 解析の実行操作、進捗・診断・結果の表示。 |
| `断面案の検討` | 断面案の作成・比較・採用。 |
| `荷重・重量・質量` | 荷重条件、重量・質量の算定と扱い。 |
| `材料・断面諸量・弾性剛性` | 材料特性、断面諸量、弾性剛性の算定。 |
| `非線形モデル・復元力` | 材料・部材の非線形モデルと履歴・復元力。 |
| `構造解析` | 解析の定式化・解法・収束・応答の算定。 |
| `許容応力度・部材検定` | 許容応力度や各部材の検定。 |
| `部材終局耐力` | 部材・接合部の終局耐力の算定。 |
| `建物全体の耐震評価` | 層・建物全体の変形や耐力、耐震性能の評価。 |
| `数量積算` | 材料・部材の数量算定。 |
| `保存・入出力` | 保存・読込・書出と形式間のデータ交換。 |
| `外部AI連携` | 外部AIとの接続・操作・結果の受け渡し。 |
| `開発基盤` | ビルド・CI・依存関係・配布・リポジトリ運用。 |

## プルリクエスト

1. `main` から作業ブランチを作成します。
2. 変更を加え、変更範囲と影響範囲をローカルで検証します。
3. [Git コミットルール](#git-コミットルール) に従ってコミットします。PR タイトルも同じ書式です。
4. `main` 向けに PR を作成し、[PR テンプレート](.github/pull_request_template.md) に従って記録します。

検証欄には実際にローカルで実行したコマンドと結果を記載し、CI の結果とは区別してください。
変更内容に対応する文書を更新し、申し送り・V&V の更新時はそれぞれの索引の手順に従います。

## 詳細情報

文書・コード・設定の正本への入口は [dev_docs/README.md](dev_docs/README.md) にまとめています。
