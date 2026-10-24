# 梁の一律鉛直偏心 — Issue #726 の確認記録

2026-10-24。対象は節点・支持アンカー基準の一律鉛直偏心、有限実配置、GUI入力・形状表示、保存と編集履歴である。

## 独立期待値と直接検証

- core `model::beam_placement::tests`：矩形RC/SRCの節点Z=−500、せい600に対し偏心0/+100/−100の上下面は−200/−800、−100/−700、−300/−900 mm。せい700への変更は材軸不変・上下へ50 mmずつ。
- 斜梁の節点Z=−500/−400、偏心+100で実端Z=−400/−300、中央−350 mm。元の高低差・傾斜・材軸長を保つ。
- 30度の矩形梁の同一XY上下差は半せい300/cos30°=346.4101615138 mm。始端XYでは有限端面で下端を材軸Zに切り、張出しの先端への一点接触も交点として返す。
- 非対称T形（H600/B400/tw20/tf20）の図心は下端から412.4489795918 mm。上下端を一律±300にしない。断面を90度回転した矩形は幅/せいの方向を交換する。
- 円管の上下端・接線交点、非有限な問い合わせ、偏心欠落・不正寸法・形状欠落を検査する。
- 基準節点移動への追従、SLだけの変更で不動、支持大梁と上下に離れた小梁の支持アンカー保持、保存入力からの偏心欠落と明示0の区別を確認する。
- App `app::beam_placement_tests`：GUI編集経路、準備/解析/検定の失効と計算入力識別の変更、Undo/Redo、OVIKAの偏心−100/0/未指定の保存再読込、節点・断面編集時の実配置更新を検査する。
- 同一外力で偏心の保存値だけを変えた線形解析の変位・部材力を比較し、共有節点・接続条件と解析応答が不変であることを検査する。
- Viewer `viewer::beam_placement_tests`：形状の実材軸線とピック、小梁の非節点端部表示、偏心大梁・偏心小梁の実材軸上からの支持アンカー選択と確認丸印を検査する。保存する支持部材ID・材軸位置と基準位置Nを保持し、解析モードはNを使う。

## 実行記録

初回の幾何試験6件では5件成功。不正断面のfixture構築自体が先に失敗した1件を、生成後に形状を不正化する入力へ修正した。
GUI構成の初回ビルドでは、共通fixture `test_fixtures/static595.rs` と `rc_wall617.rs` の追加フィールド不足を検出・是正した。

修正後の `cargo test -p sepika-core -p sepika-app --features sepika-app/gui beam_placement --locked` は、core 7件・App 3件が成功した。この記録は支持アンカーピック修正前の実行を指す。
支持アンカーピック修正後の `cargo test --workspace --features sepika-app/gui,sepika-mcp/mcp,sepika-io/parquet --locked --no-fail-fast` は exit 0、103試験群で4450件成功・失敗0・ignored 4（doc-testsを含む）だった。App GUIは904件、full_modelは37件、wall_modelは16件が成功し、偏心大梁・偏心小梁の支持先選択も成功した。
この横断実行は、不正アンカー位置を直接・間接に検査する追加ガードの導入前にcoreをビルドした時点の証拠である。追加ガード後の直接受入と静的解析は別に記録する。

追加ガード後の最終sourceに対して `cargo test -p sepika-core -p sepika-app -p sepika-io --lib --features sepika-app/gui,sepika-io/parquet beam_placement --locked` は exit 0、core 7件・App/Viewer 4件の計11件が成功した。支持アンカーの t=−0.25/1.1/∞/NaNを、直接および支持小梁を介する間接参照の両方で拒否し、問い合わせ対象IDと不正な支持小梁ID・理由を確認した。

最終sourceの追加検証はいずれも成功した。

- `cargo test -p sepika-io --locked`：15試験群、240件成功・失敗0・ignored 2。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`。
- `cargo clippy -p sepika-app -p sepika-mcp -p sepika-io --all-targets --features sepika-app/gui,sepika-mcp/mcp,sepika-io/parquet --locked -- -D warnings`。
- `cargo fmt --all -- --check`、`cargo run -p xtask --locked -- check-docs`、`mdbook build`。

ツールチェインは `rustup update stable` により最新の rustc 1.99.1、Clippy 0.1.99（2026-10-13）を確認した。
ログは実行環境のTEMPディレクトリの `sepika726-all-features-test.log`、`sepika726-final-acceptance.log`、`sepika726-io-default-test.log`、`sepika726-clippy-default.log`、`sepika726-clippy-features.log` に分けて保持した。中間sourceの横断結果と追加ガード後の検証は合算して最終headの全CI成功とは扱わない。

## 確認限界

偏心接続の剛性・内力・反力への反映（#734）、床板自重の重複控除（#616）、嵩上げ重量（#731）、床板偏心（#727）、STB偏心取込みは確認対象外。
この記録は直接Verificationであり、手動GUI操作、実施工や実測とのValidation、公開PRの最終CI成功を先取りしない。
