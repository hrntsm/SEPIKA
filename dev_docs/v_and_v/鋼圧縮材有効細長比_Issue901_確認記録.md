# 鋼圧縮材の有効細長比上限（Issue #901）

記録日: 2026-10-27。状態: 🔶。通常鋼の公開検定・表示・保存経路の有限Verificationであり、全規準適合、実験Validation、専門家サインオフ、手動GUIを意味しない。

## 根拠と適用範囲

2025年版『建築物の構造関係技術基準解説書』第2版第1刷（2026-02-11）、本文p175、3.6.3（分冊「第03章_構造細則/3.06_鉄骨造.pdf」PDF2頁）の原画像を確認した。令65条の圧縮力を負担する構造耐力上主要な鋼部材の柱200以下、柱以外250以下、および主軸ごとの有効座屈長/断面二次半径の最大値を対象とした。原画像・本文抽出は追跡しない。書誌・刊行状況は[#879記録](鉄骨材料と許容応力度_Issue879_原典照合.md)を参照。

公開一括検定は入力の断面力配列から既存危険断面位置を抽出し、引張正のNをそのまま検定器へ渡す。このため、応力度計算と同じ位置のN<0で圧縮を判定する。端部・中央・追加位置、各ケース・組合せを独立に扱い、別位置・別組合せの引張によって圧縮NGを相殺しない。検定位置間の未供給の内力分布を推定する機能は追加していない。上限・表示仕様の正本は[製品文書](../../docs/calc_basis/06_一次設計/03_鋼構造_断面検定.md#圧縮材の有効細長比上限)を参照。

SRC/CFTは構造種別による既存別検定器を維持する。BRBは製品Na・Lk/限界長の独立検定であり、芯材/拘束材の断面性能と令65用有効長の対応が未確認である。通常鋼λの流用・法令対象外の認定はせず、[#932](https://github.com/hrntsm/SEPIKA/issues/932)へスコープ分割した。BRB適合は令65適合を保証しない。

## 結果表現と安全性

既存の構造規定成分を用い、上限超過時にλ/上限を不適合として返す。これは応力度比ではない。確定した法定不適合を応力度未対応によるSkippedで隠さないため、超過時は応力度検定より先に返す。表示の項目フィルタでも超過した構造規定を残す。NaN・無限値・負の有効長・非正の断面性能、表現不能な二次半径/λは理由付き未判定とし、0への補完で適合にしない。座屈長さ0の既存座屈無視契約は変更しない。

許容fc式・材料E共通適用・自動Kは変更しない。自動K原典の未確認、採用fcと告示式の差、BRB、設計ルートは本是正で確認済みとしない。#489は再開しない。

## 要素と回帰証拠

| 要素 | 独立期待値・試験入口 |
| --- | --- |
| 柱200/200.0001、梁・筋かい250/250.0001 | `tests/steel_slenderness.rs::issue901_statutory_boundaries_both_axes_and_terms_ignore_tension_and_zero_axial_force`。A10000、Iy400000000、Iz100000000、二次半径200/100mm。長短期・各軸支配・N=-100000/0/+100000、N0には二軸曲げ。比率の期待値は柱1.0000005、他1.0000004 |
| 不正入力・相似 | 同ファイル`issue901_invalid_compression_inputs_cannot_become_passing_zero_slenderness`と`issue901_similar_sections_have_no_artificial_slenderness_cutoff`。scale=1e-12/1e-6/1/1e6、I/Aのoverflow/underflowも拒否 |
| SRC/CFT非流用 | 同ファイル`issue901_composite_checkers_do_not_apply_ordinary_steel_limit`。構造種別で別検定器を選ぶ |
| 公開一括・位置混在 | 同ファイル`issue901_public_batch_retains_compression_failure_among_tension_and_zero_positions`。端引張/中央圧縮/他端0を独立に判定。`issue901_public_batch_uses_face_positions_without_node_center_axial_substitution`ではフェース0.1/0.9・中央0.5を判定し節点芯を除外 |
| 全フィルタ部材集約 | `viewer/check_ratio.rs::issue901_failed_provision_survives_all_filters_and_mixed_positions`。適合曲げ2位置と超過1位置を集約して全項目でNG |
| App・OVIKA・旧検定失効 | `app/result_contract_tests.rs::issue901_ovika_current_failure_roundtrips_and_old_design38_expires_until_recheck`。実解析、実保存・読込、検定38失効と39再検定、解析応力維持 |
| 荷重組合せ | 同ファイル`issue901_load_combinations_keep_tension_and_compression_outcomes_separate`。G+P±Eの実求解で圧縮NGとなる柱が正負で入れ替わる |
| 自動egui | 同ファイル`issue901_egui_table_displays_statutory_failure`。表の上限理由・構造規定・NGの描画文字を検査 |

## 実行結果

以下の全体回帰はmain取込前の`3409021f`の変更を対象とする。全cargoで専用`CARGO_TARGET_DIR`を指定。`rustup update stable`後、rustc 1.99.1 / clippy 0.1.99を確認した。

- `cargo test -p sepika-design-jp --locked`: 単体658件・統合27件（新規`steel_slenderness`6件を含む）、合計685件成功。doc-testは対象0件で、新規試験の成功件数へ算入しない。
- `cargo test -p sepika-app --lib --features gui --locked`: 990件成功。
- `cargo test -p sepika-app --lib --features gui issue901 --locked`: 新規4件成功（0件filterを成功扱いしない）。
- `cargo clippy -p sepika-design-jp -p sepika-app -p sepika-core --all-targets --features sepika-app/gui --locked -- -D warnings`: 成功。
- `cargo run -p xtask -- check-docs`: summary124・links663・impl refs707で成功。
- `cargo fmt --all --check`、`mdbook build`、`git diff --check`: 成功。


### main統合後の確認

先行#923（main `8a1df66d`、検定契約38）を`eface27c`で取り込み、解析63・検定39を維持した。統合後に上記の`steel_slenderness`6件とAppの`issue901`4件を再実行して全成功。3クレートの同じclippyコマンド、fmt、mdBook、diff検証も再成功した。`check-docs`はsummary124・links664・impl refs707。main取込を理由に無関係なcore全試験を繰り返さず、全体回帰と統合後の直接再検証を区別した。

## 残る未検証

物理実験・実建物Validation、手動GUI、全認定鋼材・全製品適用条件は未確認。BRBは#932、自動有効長の根拠とfc採用式の差は#879/#697の記録を維持する。
