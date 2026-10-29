# 結果から計算時入力への往復（#978）

## 対象と状態

[Issue #978](https://github.com/hrntsm/SEPIKA/issues/978)と[PR #996](https://github.com/hrntsm/SEPIKA/pull/996)の、静的変位・既存主架構検定・数量から計算時の対象、条件、入力へ戻る経路を扱う。状態は🔶。2026-10-29にmain統合後の自動検証と最終実行ファイルによる代表・古い・取得不能結果の有限な実画面往復を確認した。全モデルの網羅確認や構造計算のValidationを意味しない。

これは参照先・表示・入力固定・失効のVerificationである。静的解析解、検定式、数量算定式の新たな数値照合や、実建物・実験とのValidationは対象外とする。新検定式、全解析種別のスキーマ統一、MCP公開、#979の比較案・検討保存は検証しない。

## 証拠の区別

以下の自動検証は、コードHEAD `62ebab2b6eae87e0a4b4058404bbe2e7d3233095`についてPR #996に記録された実行履歴である。本記録を作成した担当者による再実行結果ではない。main統合後の検証結果や実画面の観測へ読み替えない。

| PRに記録されたローカル実行 | 記録上の結果 |
| --- | --- |
| `cargo test -p sepika-app --features gui --lib issue978 --locked --offline --target-dir target` | 追加14件成功、fixture生成1件ignored |
| `cargo test -p sepika-app --features gui --lib app::references::tests --locked --offline --target-dir target -- --quiet` | 38件成功 |
| `cargo test -p sepika-app --features gui --lib app::design_input_tests --locked --offline --target-dir target -- --quiet` | 8件成功 |
| `cargo test -p sepika-app --features gui --lib design_view::tests --locked --offline --target-dir target -- --quiet` | 2件成功 |
| `cargo test -p sepika-app --features gui --lib app::result_contract_tests::gui_reports_old_contract_and_explicit_recheck_button_recovers_without_reanalysis --locked --offline --target-dir target -- --quiet` | 1件成功 |
| `cargo test -p sepika-app --features gui --lib app::nonnodal_quantity_acceptance_tests --locked --offline --target-dir target -- --quiet` | 4件成功。上記と合わせ関連53件 |
| `cargo test -p sepika-app --features gui --test lip_channel --locked --offline --target-dir target` | 最終7件成功 |
| `cargo test -p sepika-app --features gui --lib --locked --offline --target-dir target -- --quiet` | readonly数量入力表示の最終拡充前1075件成功、fixture1件ignored。最終差分全体の再実行結果ではない |
| `cargo clippy -p sepika-app --all-targets --features gui --locked --offline --target-dir target -- -D warnings` | 成功（stable/clippy 1.99.1） |
| `cargo fmt --all --check` | 成功 |
| `cargo run -p xtask -- check-docs` / `mdbook build` | 成功（check-docs: 124ページ・675リンク・718実装参照） |
| `cargo build -p sepika-app --features gui --bin sepika --locked --offline --target-dir target` | 成功 |

同PRは最終差分のfresh独立レビューを修正必須事項なし、同HEADのCIを全品質ゲート成功と記録する。初回CIでは汎用説明の「OK」が既存の未検定表示テストと干渉し、説明を「合否」に修正した後にlip_channel 7件とCIが成功した。この履歴もmain統合後の最終CIの代わりにはしない。

既存のWindows起動試行はComputer Useで `failed to activate captured window` が初回と対象再取得1回に発生し、入力操作は未実施とPRに記録されている。起動できたことを往復確認の成功には数えない。

## 代表入力と再生成

fixture生成入口は `crates/sepika-app/src/app/result_evidence.rs` のignored test `issue978_gui_fixtures`。作業ツリーのルートで次を明示実行する。

```powershell
cargo test -p sepika-app --features gui --lib issue978_gui_fixtures --locked --offline --target-dir target -- --ignored --nocapture
```

PRにはこの生成1件の成功が記録されている。出力先は `target/issue978-fixtures/` で、生成物は製品の計算根拠や保存履歴の追加仕様ではない。

| ファイル | 定義と用途 |
| --- | --- |
| `representative.ovika` | 内蔵門型ラーメンにケース#0の線形静的解析と主架構検定を実行した保存入力。代表結果・readonly・現行編集・元結果復帰用 |
| `missing-input.ovika` | 同じ結果から計算時入力記録だけを除去し、保存参照と設定を保持した入力。読込時の既存失効・除外規則を通る取得不能の確認用 |

門型ラーメンはスパン6000 mm・高さ3500 mm、柱脚固定、柱断面#0「柱 H-300x300x10x15」、梁断面#1「梁 H-400x200x8x13」、材料#0「SN400B」・Fy=235 N/mm²。ケース#0「長期」は梁#2に鉛直下向き10 N/mmの等分布荷重を持つ。代表節点#2の座標は[0, 0, 3500] mm。これらはfixtureを識別し画面間の同一性を確認する値であり、解析値の独立期待解ではない。

計算時入力欠落時は読込処理が静的・検定結果を除外し得る。欠落した詳細を画面で復元できることは期待せず、取得不能・要再計算の案内と、現在入力で補完しないことを確認する。

## 実画面の一次観測

2026-10-29の10時台JSTに、実機担当が下記コードHEADの通常可視exeで操作した観測を受領した。本記録の文書担当によるGUI再実行ではない。最終fixture（10:02:32 JST再生成）を「ファイル」→「開く…」から読み込んだ。representativeは自動準備後に部材5・節点6・断面2、入力世代0・Undo無効・最新を表示した。次の数値は画面間で同じ記録を追跡した証拠であり、独立計算による期待解ではない。

| 項目 | 操作と観測 | 状態 |
| --- | --- | --- |
| 代表検定の往復 | 設計・検定表の部材#0、ξ=0、比0.0967「良好」から根拠へ移動。世代0・実行c67b7118-68b8-4940-af10-cbb7efc00dee・契約44、比較集合1件と長期#0 Deadの支配ケースを確認。readonlyを閉じて根拠へ戻った | 確認 |
| readonly | 材料SN400B・Fy=235 N/mm²、断面#0 H-300x300x10x15・主材料割当0・寸法300/300/10/15を実画像とアクセシビリティ情報で確認。閲覧後も世代0・Undo無効・最新を維持 | 確認 |
| 現行編集と旧入力 | 根拠窓「現在の材料入力を編集」→現行材料#0のFyを235から236へ置換しEnter確定。世代1・Undo有効・staleとなった。固定入力を再表示するとFy=235・世代0を維持し、現在世代1を区別した。再計算・保存なし | 確認 |
| 古い結果と復帰 | 保持根拠は比0.0967と旧Fy235を維持し「履歴・要再計算」「静的0の生成入力不一致」を表示。「元の結果へ戻る」で設計・元単独配置・元検定表へ復帰。根拠を閉じた通常表は要再計算で比の通常行を表示しなかった | 確認 |
| 静的変位 | 編集後の結果工程で現在選択を旧入力へ流用しない案内を確認し、計算時節点#2 [0,0,3500] mmを選択。長期#0 Dead・全体座標X/Y/Z [mm]、Z=-0.043241 mmを根拠・元画面で確認。readonlyの節点#2座標と保存解析条件を確認後に閉じ、元値・世代1へ復帰 | 確認 |
| 数量 | 世代1でモデル全体の鋼材体積0.132938 m³・鉄骨1.044 t、根拠で同体積・鉄骨1.043563 t・明細3件・数量契約1・解析検定成功と別の状態を確認。readonlyの数量設定・配置支持範囲を開いて閉じ、元数量画面へ同値・世代1で復帰 | 確認 |
| 取得不能 | missing-input読込後は世代0・Undo無効・stale、静的入力識別情報なし・再実行要求、静的ケース0・検定0・未計算/結果なし。検定表は計算時入力取得不能、保存版欠落/現行44、要再計算/最新利用不可と検定結果なしを表示し、現在入力から古い値を補完しなかった | 確認 |

Fy変更はGUI往復と失効を観測するための一時操作で、変更後モデルの設計採用や検定値の妥当性確認を目的としない。名前だけの変更による世代更新と、計算入力の変更による失効は区別する。

準備段階ではFy変更を検定のみの失効と想定したが、実観測では静的解析入力も不一致となり、静的再実行を要求した。この確認記録では実際の既存失効規則による表示を採用する。

検定根拠では式名と、軸曲げσax=2.5327・σbX=6.7383・fc=47.9705・fbX=153.4692、せん断τ=3.1538・fs=90.4515・比0.0650の数値内訳を確認した。別の保存詳細がない欄は「既存詳細は記録がなく取得不能」と表示し、内訳があることから欠落詳細の存在を推定しなかった。

数量の明細は柱#0/#1各0.041458 m³・梁#2 0.050022 m³。readonly数量設定は壁筋径10・定着35・主筋比0.008・スターラップ比0.001・床筋比0.01。配置支持範囲は展開できたが、この門型入力に床・壁・二次部材はなく空であり、実配置を持つモデルの実機確認はしていない。

静的readonlyには保存解析条件C0=0.2・Z=1・地盤II・AiApprox・CorrectedLumped・雪false・δ1=0.7・δ3=0.35・thread=0を表示した。下部パネルを閉じ、単独表示にして読める状態で確認した。

最大化直後のアクセシビリティ対象のずれは通常窓へ復旧して安定した。曖昧な操作対象や全選択値の読み違いによる自動承認の拒否は、同じ操作を再試行せず、対象再取得・正確な操作名・ラベル付き値とスクリーンショット確認で解消した。OS設定の取得・起動・変更やユーザー追加許可で回避していない。通常Closeで終了後、freshウィンドウ一覧とプロセス一覧に専用exeがないことを確認。標準出力・標準エラーは日本語フォントのinfo 1行のみでpanic/errorなし。終了コードは取得しておらず0とは判定しない。ユーザーファイルの保存なし。

## main統合後の自動検証

2026-10-29にmain `6624fd8b6003d94a3ec722951f67eeabc3225a5c`（#976・#977を含む）を統合した。コードHEADは `bdab2a065a1b76225801e61f7697d398c294580d`。検定比から根拠を開くボタンと、共通テーマの可読文字色・良好／注意／超過の状態名を両立して競合を解消した。根拠窓・readonly窓は独自の文字サイズ・余白・色を指定せず、標準Windowのframeと共通global styleを使う。

本節は本記録を更新した担当者による実行結果であり、前節のPR履歴と分ける。

| 実行 | 結果 |
| --- | --- |
| `rustup update stable` / `cargo clippy --version` | stable rustc 1.99.1 / clippy 0.1.99 |
| `cargo test -p sepika-app --features gui --lib --locked --offline --target-dir target -- --quiet` | 1088件成功・失敗0・fixture生成1件ignored。issue978・references・design_input・result_contract・nonnodal_quantityを含む |
| `cargo fmt --all --check` | 成功 |
| `cargo run -p xtask -- check-docs` | 成功（124ページ・675リンク・720実装参照） |
| `mdbook build` | 成功 |
| `cargo clippy -p sepika-app --all-targets --features gui --locked --offline --target-dir target -- -D warnings` | 成功 |
| `cargo build -p sepika-app --features gui --bin sepika --locked --offline --target-dir target` | 成功 |
| `cargo test -p sepika-app --features gui --lib issue978_gui_fixtures --locked --offline --target-dir target -- --ignored --nocapture` | 1件成功、10:02:32 JSTに両fixtureを最終コードで再生成 |
| `cargo test -p sepika-app --features gui --test lip_channel --locked --offline --target-dir target` | 実機終了後の再実行で7件成功・失敗0 |

このコードHEADのGUI実行ファイルは作業ツリー内の `target/debug/sepika.exe`（51,297,792 bytes、2026-10-29 09:58:14 JST生成）。実機確認ではこの実行ファイルを用い、別作業ツリーのバイナリを代用しない。

実機担当が記録した起動exeのSHA-256は `1EFAD110EEBD94DEFFECD6A42BB564CFE80CBA274C717D71BC23027512BE3350`。ビルド成功後に開始したlip_channelのcargo実行は、起動中の同exeを再ビルドしようとして削除時に `Access is denied. (os error 5)` で失敗した。起動中プロセスは停止せず、実機終了後に再実行して7件成功した。一時的な実行ファイルのロックであり、現役の阻害要因は残っていない。

fresh独立レビューはコードHEAD `bdab2a06` のmainに対する全差分を確認し、修正必須事項なし。Reviewer自身のissue978テストは当初ロック待ちだったが、後にsession 20218を回収して追加14件成功・fixture1件ignored・終了コード0を確認したとの報告を受領した。文書担当による再実行とは区別する。

## 最終実行と残る範囲

上記の有限なGUI往復確認を完了した。最終文書のfresh独立レビューと最新HEADのCIは別途確認する。表示倍率100%・150%のユーザー確認を、この機能の往復確認へ代用していない。

全モデル・全材料・全検定位置・全ウィンドウ幅・全入力方式・支援技術を網羅する確認ではない。結果が有効であることと個別検定の合格を分離し、記録のない式や数値を推定していないことを確認する。#979の保存接続は後続機能の実装と検証を必要とし、この記録だけで接続済みとは扱わない。
