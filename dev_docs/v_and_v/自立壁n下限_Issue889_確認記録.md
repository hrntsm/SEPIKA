# 自立壁n倍法の係数下限（Issue #889）

2026-10-27、main `440654c7` を基点に[Issue #889](https://github.com/hrntsm/SEPIKA/issues/889)を確認した。採用範囲の判断は[ADR 0083](../adr/0083-misc-wall-coefficient-lower-bound.md)、製品仕様は[形状係数Fes](../../docs/calc_basis/07_二次設計/04_形状係数_Fes.md)を正本とする。[#620の点検](自立壁n倍法_Issue620_確認記録.md)と[#740のRs接続保留](自立壁Rs接続_Issue740_調査記録.md)は維持する。

## 原典照合と結論の限界

2025年版『建築物の構造関係技術基準解説書』**第2版第1刷**の原画像を直接確認した。奥付は2025-06-30第1版第1刷、2026-02-11第2版第1刷を表示する。先行#740の照合には版・刷の記載がなかったため、今回確認した版を区別する。原画像・本文抽出・ローカル保管先・ハッシュはリポジトリへ保存しない。

| 範囲 | 印刷頁／各分冊のPDF頁（1始まり） | 確認した事項 |
| --- | --- | --- |
| 表紙・奥付 | 表紙／1、奥付／1 | 年版・書名と上記の版・刷 |
| 目次 付録1-3.1 | 目次8／16 | 部材の力学モデルの技術資料は695頁から |
| 付録1-3.1(9)④、式(付1.3-58) | 740／46 | Dw=nAwDc/Ac、nは1以上。DcとAcは同じ代表的内柱のD値と断面積。支持条件・配置等に応じ危険側にしない係数設定が必要。一律推奨nなし |
| 同④の続き | 741／47 | 仮定剛性をRe/Rsに考慮する必要。応力計算へ原則算入しない。現行量へのスカラー接続式の指定ではない |

[ICBA書誌](https://www.icba.or.jp/kenchikuhorei/)で2025-06-30刊行と公開リンクを確認。[正誤表（第1版第1刷用、R8.1.22）](https://www.icba.or.jp/zzfilebox/kenshuka/2025seigo.pdf)に740–741頁の訂正はなく、[Q&A（R8.3.27）](https://www.icba.or.jp/zzfilebox/kenshuka/2025qa.pdf)にもn<1を認める根拠は確認できなかった。第1版向け正誤表を第2版本文の代用にせず、第2版の該当本文を直接確認した。全版の差分照合や2020年版との本文同一性は確認していない。

公開元を再確認した採用例は、[層内合算の計算資料 §(7)](https://kke-resp-team.github.io/resp-d-docs/calculation2/03_stress_analysis/03_eccentricity_ratio/index.html)、[公開Q&A（URL1539）](https://support.kozo.co.jp/support/disp.php?p1=1539&q2=bus)、[公開Q&A（URL3736）](https://support.kozo.co.jp/support/disp.php?p1=3736)、[指定柱方式の計算資料 §2.4.2、本文36／PDF39](https://www.structure.jp/manual/be513.pdf#page=39)。いずれも確認した範囲に1未満を明示的に許容する説明はない。係数を入力値とする記述だけでは1未満の根拠にしない。網羅的に「世の中に根拠がない」と証明したものではない。

原典の代表内柱Dc/Acは、現行の柱別|Q|/|δ|を層内合算するΣKc/ΣAcと無条件に同値ではない。公開採用例は存在するが、D→Kの基準剛性・評価状態、代表内柱から層内合算への置換条件は未確定。この不足を下限採用によって解消した扱いにしない。新近似を導入せず、Rs接続の適否は#740に残す。

## 独立例と安全性の判定

一貫単位N–mmで、柱4本を(0,0)、(6000,0)、(0,6000)、(6000,6000)へ置き、各柱のX/Y剛性を各1 N/mm、総柱面積を400 mm²とする。壁は(0,0)–(6000,0)、Aw=100 mm²、X方向だけへ働く。現行式に対して壁剛性はn N/mm。これは現行式のVerification用の架空数値で、壁の物理Validationではない。

独立算術は `fractions.Fraction` で、剛心Ys=12000/(4+n)、Xs=3000、KR=72000000+36000000n/(4+n)を計算した。重心はXg=3000固定、Ygを3000又は1000 mm、Rs=1固定とする。

| n | Ys [mm] | KR [N·mm] | Re(X)、Yg=3000 | Fes、Yg=3000 | Re(X)、Yg=1000 | Fes、Yg=1000 |
| --- | --- | --- | --- | --- | --- | --- |
| 0.5（独立算術だけ） | 8000/3 | 76000000 | 0.0811107106 | 1 | 0.405553553 | 1.5 |
| 1 | 2400 | 79200000 | 1/(2√11) ≈ 0.150755672 | 1.002518908 | 7/(6√11) ≈ 0.351763235 | 1.5 |
| 2 | 2000 | 84000000 | 1/√14 ≈ 0.267261242 | 1.390870806 | 1/√14 ≈ 0.267261242 | 1.390870806 |

n=0.5から1への変更で付加剛性は倍増するが、第1例のFesは増加、第2例は上限で不変。受理範囲内のn=1→2では第1例のRe/Fesが増え、第2例は減る。同じDs・Qudなら必要耐力はFesに比例するが、保有耐力の実建物通し実行の証拠ではない。したがって、下限変更やn増加を一律に安全側とは断定しない。Rsの再接続・正規化は試しておらず、#740の独立連成反例とRs/Fs算術を本件の製品実行と混同しない。

## 要素から製品経路への証拠索引

| 要素・経路 | 試験・読取証拠 | 観測対象と限界 |
| --- | --- | --- |
| 係数単体の下限 | `eccentricity::tests::misc889_coefficient_lower_bound` | n=1、2の独立値。正の最小値・0.5・1直下・0・負・NaN/±Infを拒否 |
| モデル検査、JSON/MessagePack | `model::stress_cfg::misc889_tests::misc889_input_serialization_and_model_validation_share_lower_bound` | None/1/2を保持、欠落は既定None。不正値の直書き・書出・読込を拒否。非有限値をJSON nullへ変えない |
| OVIKA保存・読込 | `ovika::reference_tests::misc889_ovika_rejects_invalid_n_without_replacing_saved_model` | None/1/2保存読込、範囲外と非有限の保存失敗時に既存ファイル・入力値を保持。正しいハッシュ付き旧n=0.5ファイルを読込拒否し原ファイル保持 |
| 壁なしの実行 | `eccentricity::tests::misc620_invalid_coefficient_is_rejected_even_without_walls` | 壁なしでも0.5/0/負/NaN/±Infを拒否、Noneは通過 |
| 付加→剛心→Re→Fes | `eccentricity::tests::misc889_wall_coefficient_can_increase_or_decrease_re_and_fes` | 上記n=1/2の壁投影・剛心・KR・2重心のReを独立値と照合、Fesの両方向変化。0.5は追加前に失敗し剛性列を保持 |
| 既存3D質点系 | `lumped_mass::tests::misc620_spatial_propagates_invalid_n_and_missing_column_response`、`misc620_existing_spatial_column_ki_includes_wall_but_planar_does_not` | 不正係数の伝播とn=1の既存ColumnKi付加を確認。新しい動的応答の妥当性確認ではない |
| GUI/CSVの結果依存 | `summary`と結果契約関連試験 | 共通層指標入口・入力対応・契約版gateを確認。app全体を検索した結果、係数nの編集欄は存在せず新GUIを追加しない。手動GUI未実施 |

OVIKAの読込は拒否で終了しモデルを返さず、保存は一時書込前の直列化で拒否するため既存ファイルを置換しない。旧範囲外ファイルをGUIで開いてnだけ修正する回復経路はない。元ファイルの保持とエラーを優先し、自動補完・移行や「考慮しない」への切替は行わない。#620で確認した有効な剛性0・欠測応答の区別は維持する。

解析/検定契約版の変更理由とスキーマ版の扱いはADR0083を参照。旧成果の同梱・モデル内の保存解析設定等も新契約へ再刻印しない。

## ローカル検証

全Cargoコマンドは `CARGO_TARGET_DIR=C:\Users\hiroa\Desktop\ikkan\target` で実行した。

- `rustup update stable`：成功。rustc 1.99.1（2026-10-13）、`cargo clippy --version` はclippy 0.1.99。
- `cargo test -p sepika-core -p sepika-io -p sepika-design-jp -p sepika-job --locked misc889`：初期の境界・入出力3試験成功（Re/Fesの影響試験は追加前）。
- `cargo test -p sepika-core -p sepika-io -p sepika-design-jp --locked --lib`：全成功。Re/Fesの影響試験、壁なし係数検査、既存#620の有限試験を含む。IOの既定ignored 1件は未実行。
- `cargo test -p sepika-job --locked --lib lumped_mass`：33件成功。
- `cargo test -p sepika-app --locked --lib summary`：17件成功。
- `cargo test -p sepika-app --locked --lib result_validity`：22件成功。
- `cargo clippy -p sepika-core -p sepika-io -p sepika-design-jp -p sepika-job -p sepika-app --all-targets --locked -- -D warnings`：成功。
- `cargo run -p xtask -- check-docs`：成功（summary 124、links 659、impl refs 706）。`mdbook build`：成功。
- 変更した開発文書6件の相対リンク811件・見出しアンカー111件を確認し欠落0件。`cargo fmt --all --check`、`git diff --check`：成功。

製品試験に失敗はなかった。Rustの変更経路と直接利用側の有限Verificationであり、CI・独立レビュー・最終main統合は別ゲートで確認する。

## 未確認と対象外

全層内合算方式の原典適合、全モデルでの誤差上限、2020年版原書、手動GUI、実験・実測Validation、専用2D/3D動的比較は未確認。指定柱・Ke・開口/スリット・一律推奨n・解析再設計は対象外。#740・#620の残件を本件で完了扱いにしない。
