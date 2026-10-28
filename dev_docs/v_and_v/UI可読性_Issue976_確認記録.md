# GUI可読性の確認記録（#976）

作成日: 2026-10-28。状態: 🔶（描画変更のVerification、必須実機条件は未了）。
共通規約は[GUIの文字と状態表示](../ui_readability.md)を参照。

## 最新の統合ソースによる自動確認

コードコミット `a3528f51` で、#975を含むmain `104a50bbe231e74e59942bc12017cace782dc11f` を取り込んだ。主領域の配置・狭幅での領域切替・領域見出し、ツリーの部材グループに共通の選択印とフォーカス輪郭を適用した。統合ではグリッドの編集IDとフォーカス追跡を保持し、表示変更から入力の確定処理を変更していない。

| 検証 | 実行結果と範囲 |
| --- | --- |
| `cargo test -p sepika-app --features gui --lib --locked` | 全1067件成功。コンパイル1分48秒、試験7.62秒。新主領域の選択印、配置・切替時のモデル/結果/Undo不変性、検定比の失効表示、文字コントラストを含む |
| `cargo clippy -p sepika-app --features gui --all-targets --locked -- -D warnings` | 成功、14.31秒 |
| `cargo build -p sepika-app --features gui --bin sepika --locked` | 成功、1分05秒。専用worktreeの `target/debug/sepika.exe` を生成 |
| `cargo fmt --all --check` / `git diff --check` | 成功 |
| `cargo run -p xtask -- check-docs` | 成功。summary 124、links 674、impl refs 716 |

これらは統合後の自動検証であり、100%/150%の実機表示・キーボードフォーカス・IMEの観察を代替しない。以下の旧headの結果は、実行時点を区別して保持する。

## 過去の実行と確認範囲

- 先行#974取り込み前: `cargo test -p sepika-app --features gui --lib --locked` は1011件成功。新規の文字コントラスト試験と検定状態境界試験を含む。これは旧headの結果であり、#974取り込み後の検証と区別する。
- `rustup update stable` はstable 1.99.1で更新なし。`cargo clippy --version`: clippy 0.1.99 (58cf8f9c35 2026-10-13)。
- `git diff --check` 成功。
- #974 main `f20dc6ec94ea1984fc2ac73b07c4c753cf512282` を専用ブランチへ取り込み済み。取り込み後のGUIビルドと局所検証は下記のとおり。
- `cargo build -p sepika-app --features gui --bin sepika --locked` 成功（3分08秒）。専用worktree `target/debug/sepika.exe` を起動し、UIAでナビゲータ・属性・共通参照などの新旧統合された表示項目を取得した。これは下記の視覚確認が成立した証拠にはしない。

- #974取り込み後、最終の表示文言修正前に `cargo test -p sepika-app --features gui --lib --locked` を実行し、1049件成功（25.22秒）。これは最終ソース全件の成功ではない。最終の表示文言修正後は `cargo test -p sepika-app --features gui --lib --locked theme::tests` の8件が成功。160pt幅で長い名称の描画矩形が収まることを製品scale 1.0/1.5で検証したが、これはヘッドレス配置試験でありOSの100%/150%実機表示ではない。
- 最終差分に対する `cargo clippy -p sepika-app --features gui --all-targets --locked -- -D warnings` 成功（18.45秒）。
- 最終文書に対する `cargo run -p xtask -- check-docs` 成功（summary 124、links 671、impl refs 713）。開発文書の相対リンク先も存在確認済み。

## 実機条件と未確認

| 条件 | 実測・観察 | 想定/未確認 |
| --- | --- | --- |
| OS | Windows上の専用worktreeでビルド | OSの版・画面解像度は未取得 |
| OS倍率 | レジストリ `WindowMetrics/AppliedDPI=168`、`Win8DpiScaling=0` を読み取り | 168/96=175%相当は計算値。実効DPI・現在の設定画面上の値とは断定しない |
| 対象ウィンドウ画像 | 取得画像1282×827 px。内容は壁紙のみでアプリを描画した画像ではない | この寸法は画面解像度・アプリ有効寸法の実測に使用しない |
| 解像度 | `Get-CimInstance Win32_VideoController` はAccess denied | 画面寸法の実測なし |
| Windows設定画面 | `sky.launch_app(SystemSettings.exe)` は約14分無応答となり中断。設定のクリック/入力は一度も実行していない | 設定値の実機観察なし |
| 復元 | DPI/解像度/製品倍率を変更していないため復元対象なし | 復元操作が成功したとの記録にはしない |
| 製品倍率 | 製品の倍率変更なし | 起動後の実効値は未取得 |
| 文字 | themeの論理文字指定: Body/Button 14pt、Small 12pt、Monospace 14pt | 実効ピクセル寸法の測定なし |
| 100% / 150% | 未実施 | 両条件のツリー・入力表・属性・結果詳細の実表示、切れ/重なり、フォーカスは未確認 |
| 入力 | キーイベントと編集確定経路は変更せず既存lib試験成功 | 実機Tab/Shift+Tab/Enter/Escと日本語IME未確定文字列の操作は未確認 |

## GUI操作が成立しなかった経緯

専用製品exeの起動・一覧・アクセシビリティ取得は成功した。`get_window_state`の画像には壁紙のみが表示され、`activate_window`は `failed to activate captured window` で失敗した。手順に従って最新の一覧から対象ウィンドウを再取得し、状態取得後に一度だけ再試行したが同じエラーとなった。古い座標でのクリック・入力は行わず、実機操作を停止した。別アプリの終了やファイル上書きはしていない。

必須の100%/150%代表表示・フォーカス・IMEを実施して記録するまで#976の完了/merge判定に使わない。ヘッドレス試験を実機DPI確認の代用にしない。


使用したスキル: `C:/Users/hiroa/.codex/plugins/cache/openai-bundled/computer-use/26.1021.31345/skills/computer-use/SKILL.md`。
同スキルが必読とする `C:/Users/hiroa/.codex/plugins/cache/openai-bundled/computer-use/26.1021.31345/docs/guidance.md` のRecoveryには次の指示がある。

> If state capture or window activation fails, stop using prior coordinates or element indexes. Refresh the app/window selection and retry once; report the exact error if recovery fails.

初回と新しい一覧/状態取得後の再試行1回で前面化が失敗したため、同じ失敗経路を反復していない。前面化できない専用instanceには終了のキー入力も送っていない。起動を確認した専用exeは残っている。設定画面のtargetable windowは一覧に現れず、他アプリには入力していない。壁紙画像を検証済み画面やPRの証拠画像として添付しない。


停止理由は製品の実表示を取得できず、必須の100%/150%・フォーカス・IMEの検証が成立しないためである。上記Recoveryの引用を、あらゆるUI操作を一律に禁止する規定とは解釈しない。専用instanceの終了についてAPIリファレンスを確認したが、前面化不要のclose/quit/terminate専用APIは記載されていない。製品の画像座標も得られていないため、閉じるボタンへの座標操作や終了キーの再試行は行わず、instance残存を引き継ぐ。


## CIで検出された表示テストの期待値不一致

head `5c635ea4` のCI run `40550296656` で、`design602_actual_table_hides_old_ratios_after_th` が失敗した。テストは変更前に表示するラベルを `0.8000` と完全一致で検出していたが、最終の画面は `0.8000 良好` を表示するため、最初のassertが成立しなかった。上記1049件の全件検証はこの表示文言修正前であり、最終headの回帰成功として扱えない。

修正では入力変更前の表示に `0.8000 良好` を要求する。入力変更と時刻歴実行後は、描画文字の空白区切りトークンに `0.8000` が一つも存在しないことを検証する。古い数値に状態文字が付いて残る場合も失敗させ、要再計算の理由表示を確認する既存のassertも維持する。計算値・結果の有効性判定は変更していない。


今回の期待値修正後の最終ソースで実行した確認:

- `cargo test -p sepika-app --features gui --lib --locked app::design_input_tests::`: 当該表示テストを含む8件成功（1041件は対象外）。
- `cargo test -p sepika-app --features gui --lib --locked app::design_input_tests::design602_actual_table_hides_old_ratios_after_th -- --exact`: 1件成功（1048件は対象外）。
- `cargo clippy -p sepika-app --features gui --all-targets --locked -- -D warnings`: 成功。
- `cargo fmt --all --check`、`git diff --check`、`cargo run -p xtask -- check-docs`: 成功。

この追試ではlib全1049件を再実行していない。上記8件と当該1件の最終ソースでの成功を記録し、過去の全件成功で最終headの全件回帰を代用しない。必須実機条件の未確認は継続している。
