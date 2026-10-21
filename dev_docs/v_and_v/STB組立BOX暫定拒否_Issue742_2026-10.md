# STB組立BOXの暫定拒否（Issue #742）

## 対象と境界

[Issue #742](https://github.com/hrntsm/SEPIKA/issues/742)の先行対応を対象とする。
元Issue本文と[#683](https://github.com/hrntsm/SEPIKA/issues/683)の調査記録をGitHub connectorで独自に全文取得した。
実行日2026-10-21、基点`8cdbe874c652919755266dc3b5fc316fce89f2cf`。
現在の受理・拒否仕様は[製品文書](../../docs/model_io/02_ST-Bridge_形式.md#組立boxの読み込み)を正本とする。
内部両板厚・性能・重量・保存・編集の本対応は#683が所有し、この先行拒否の完了で本対応を完了扱いにしない。
CFT密度#741の変更は基点に含まれ、本変更では触れない。

## 変更前の公開再現

製品ソース未変更の基点で、一時example `issue742_before` を実行した。
入力全文は恒久fixture `crates/sepika-io/tests/fixtures/cft_standard_steel.stb`、追加実行は`built_box_standard.stb`。
A=300、B=200、t1=9を固定し、t2だけを9・6・12・欠落・0・-1・bad・NaN・INF・151に置き換え、
`import_stbridge` と `import_stbridge_with_report` をそれぞれ呼び出した。

初回はCFTの20観測を実行し、同厚・異厚・不正の全条件で厚さ9のCftBoxとして成功した。
続いて鋼梁fixtureも加えた40観測を実行し、鋼梁・CFTとも全条件で同じ一様板厚・面積8676・Iy109875852・Iz58328652・J115366091.48962656を返すことを確認した。
鋼梁の単位長さ質量は0.0000681066 t/mm（68.1066 kg/m）、CFTは0.00018847920430422214 t/mmで全条件同じだった。
これは検証コマンドの正常終了と誤受理の再現であり、不正入力のテストが期待どおり拒否された意味ではない。
コマンドは `cargo run -p sepika-io --locked --example issue742_before`、終了コード0。
生ログはroot targetの `issue742-before.log`（CFT20観測）、`issue742-before-both.log`（鋼梁/CFT40観測）。
一時exampleは変更前の観測後に削除し、恒久回帰へ置き換えた。

## 独立期待値と公開入口

恒久回帰 `crates/sepika-io/tests/stbridge_built_box.rs` は両公開入口を直接呼ぶ。
鋼梁全文は `fixtures/built_box_standard.stb`、CFT全文は上記fixtureを使用する。

- 同厚300×200×9の鋭角形状、面積8676 mm²、Iy=109875852 mm⁴、Iz=58328652 mm⁴を固定期待値とする。
  外側300×200、内側282×182の矩形差による独立計算。鋼梁質量68.1066 kg/mは面積×7850 kg/m³の単位換算。
- J=115366091.48962656 mm⁴は変更前の既存性能保持を確認する回帰値であり、異厚Jの正解や新しい算定式ではない。
- 数値表記9/9.0の受理、属性報告t2の検査採用、異厚6/12と近接不等9.000000000000002の全体Errを確認する。
- A/B/t1/t2各欠落、0・負値・bad・NaN・INF・-INF・overflow表記・空文字を確認する。
- 内法高さ・幅の0/負と、有限な入力から2倍計算がoverflowする場合を確認する。
- 部材未割当の取込断面も拒否し、未参照の異厚ライブラリは正常使用断面を妨げないことを確認する。
- 同符号同階でt2だけ異なる2定義が、1断面へ統合された成功モデルとして返らないことを確認する。
- 圧延BOXの角R欠落を未知のまま受理する既存条件を対照とする。

検証は入力境界と既存性能を対象とするVerificationであり、実物の物理Validationではない。
診断とErrが同じであることも両公開入口で比較する。

## GUIの確認対象

`sepika-app/src/app/reference_identity_acceptance_tests.rs` の実読込handler回帰を別担当が追加した。
既存モデルを解析済みにし、結果・準備状態・Undo/Redo・選択・保存先等を持つ状態で一時実ファイルを読み込ませる。
拒否時の対象/理由と状態保持、正常同厚の成功差替えを確認する。
gui featureではログドックとstatus barのegui描画出力も確認対象とする。
default構成のfocused2試験成功を生ログで独自照合した。正常9/9.0の成功差替え、
異厚6/12・t2欠落/0/負/bad/NaN/INF/151の9ケースで、診断と既存状態・ファイルの保持を確認した。
GUI featureのfocused2試験成功も生ログで独自照合した。各拒否ケースでログドックの自動表示と、
実際のstatus barが返すeguiのText描画に診断が含まれることを確認した。
既存reference identity acceptance moduleのdefault11試験も成功した。
これは実handler・egui描画出力の自動試験であり、ファイルダイアログや実ウィンドウの手動視認ではない。

## 実行履歴

全Cargo実行で `CARGO_TARGET_DIR=C:/Users/hiroa/Desktop/ikkan/target` を明示した。
親担当が`rustup update stable`を実行し、rustc 1.99.1 / clippy 0.1.99（58cf8f9c35 2026-10-13）を確認した。
ローカル生ログは同targetの `issue742-*.log` に保存し、リポジトリへ追加しない。

| コマンド | 結果 | 生ログ |
| --- | --- | --- |
| `cargo test -p sepika-io --locked --test stbridge_built_box` | 7成功・失敗0 | `issue742-focused.log` |
| `cargo test -p sepika-io --locked` | 240成功・失敗0・既存ignored2。lib177＋統合63。誤統合試験へ2部材参照とt2=6/12の両異厚例を追加した最終試験を含む | `issue742-io.log` |
| `cargo clippy -p sepika-io --all-targets --locked -- -D warnings` | 成功 | `issue742-clippy.log` |
| `cargo run -p xtask --locked -- check-docs` | active worktreeで成功。summary123・links606・impl refs655 | `issue742-docscheck.log` |
| `cargo fmt --all -- --check` | 成功 | `issue742-fmt.log` |
| `mdbook build` | 成功 | `issue742-mdbook.log` |
| `cargo test -p sepika-app --lib built_box_ --locked` | 2成功・失敗0・ignored0・366filtered。default構成の実handler回帰 | `issue742-gui-default-focused.log` |
| `cargo test -p sepika-app --features gui --lib built_box_ --locked` | 2成功・失敗0・ignored0・852filtered。診断Text描画・ログ自動表示を含む | `issue742-gui-feature-focused.log` |
| `cargo test -p sepika-app --lib reference_identity_acceptance_tests --locked` | 11成功・失敗0・ignored0・357filtered。新規2試験を含む関連module | `issue742-gui-reference-module.log` |
| `cargo clippy -p sepika-app --features gui --all-targets --locked -- -D warnings` | 成功、警告・エラーなし。生ログのexit0を独自照合 | `issue742-gui-clippy.log` |

既存assert・許容差の緩和やskipは行っていない。既存ignored2は今回の変更前からの試験設定。
正常CFT密度・質量、通常の鋼/RC/SRC・配筋・標準参照・系列の既存回帰も全IO試験に含む。
新たな異厚の性能算定式や内部形状は追加していない。
`git diff --check`も成功した。

## 未確認

異厚の計算・保存・編集は#683の本対応であり未検証。
全XSD適合・他システムとの完全相互運用、実物のValidation、手動ファイルダイアログと実ウィンドウの視認は未実施。
公式寸法・XSDの再検証は本作業では行わず、#683の公開調査記録を参照した。
fresh独立レビュー・親監査・最終head全CI・通常mergeは後続の公開PRで確認する。
