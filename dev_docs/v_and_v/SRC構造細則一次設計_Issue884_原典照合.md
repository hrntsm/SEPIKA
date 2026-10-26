# SRC造の構造細則・複合断面・一次許容応力度の重点照合（#884）

確認日: 2026-10-27。対象: `7635aeed`。[Issue #884](https://github.com/hrntsm/SEPIKA/issues/884)の有限重点照合であり、全節・全式の適合確認ではない。計算コードは変更しない。状態は🔶。原典照合、独立算術によるVerification、実験Validationを区別し、実験・専門家サインオフ・手動GUIは未実施。

## 選定と原典

SRC部に固有の適用条件を落とすと、単材の許容値が正しくても安全確認が成立しないため、3.8.1–3の適用・鉄骨かぶり・準用と、6.1の応力評価範囲を優先した。材料式の再調査はせず、9.2/9.3はSRC専用の追加条件の有無を指定頁で確認する。複合断面の既存独立値は#434を参照し、今回の数値試験は公開検定入口の許容曲げ境界と適用制約に絞る。

『2025年版 建築物の構造関係技術基準解説書』の奥付原画像（`前後付/99_奥付・裏表紙.pdf`の1頁）で第2版第1刷、2026-02-11を確認。[ICBA書誌](https://www2.icba.or.jp/products/detail.php?product_id=606)と一致した。[公式情報](https://www.icba.or.jp/kenchikuhorei/)から[第1版第1刷正誤表（2026-01-22）](https://www.icba.or.jp/zzfilebox/kenshuka/2025seigo.pdf)、[Q&A（2026-03-27）](https://www.icba.or.jp/zzfilebox/kenshuka/2025qa.pdf)、[刊行後改正一覧（2026-07-01）](https://www.icba.or.jp/gijutukijunkaiseilist2025.html)を閲覧した。指定SRC細則・今回の独立算術を直接訂正する掲載は確認できない。正誤表p.568の高強度鉄筋追記は第2版の記載に存在する。Q&A No.1は設計者判断の根拠と個別建物の適用確認を求め、数値一致だけで全適合とはしない。現行法令統合本文の全面照合は未実施。

書籍は分割PDFからPNGへ描画し、下表の印刷頁と奥付を目視した。目次は位置確認に用い、照合根拠の代わりにしない。著作物の画像・広範本文抽出は追跡ファイルへ保存しない。ローカル画像は作業worktreeの`target/issue884/`に保持する。

## 原典・実装・製品文書の対応

| 節・印刷頁（分割PDF位置） | 適用条件・式・係数・単位 | 現行実装とdocs | 状態・限界 |
|---|---|---|---|
| 3.8.1 p.226（3.08の1頁）、令79条の2 | SRC造建物に加え、他構造との併用建物のSRC部分にも適用 | `SrcDesign::check`は矩形RC部・中心H形鉄骨を持つ梁/柱断面を受ける。[断面検定](../../docs/calc_basis/06_一次設計/04_SRC_CFT_断面検定.md) | **適用範囲の確認**。法令のSRC一般の範囲と、製品の対応形状を同一視しない |
| 3.8.2 p.226（同1頁）、令79条の3 | 鉄骨のコンクリートかぶり原則5 cm以上。第2項の大臣指定・認定による例外を区別 | 中心H形なら横かぶり=(b−鋼幅)/2、上下=(D−鋼せい)/2 [mm]。`SectionShape`/配筋幾何の成立検査は法定鉄骨かぶりの検定ではない | **未対応（自動適合判定）**。49.9/50/50.1 mmの公開入口受理を下記で観測。例外成立・施工性を受理結果から推定しない |
| 3.8.3 pp.226–227（同1–2頁）、令79条の4 | S/RC規定の準用。除外は令65条・70条・77条第四号。72条二号の鉄骨/鉄筋間隔とこぶ、77条六号の鉄骨/主筋の読替え | 材料・配筋の入力検査と部材検定はあるが、柱脚・接合・継手/定着・材料施工・準用全条件の一括検定なし | **未対応（全条件自動判定）**。#878/#881のS/RC事項を参照し、Sの細長比上限をSRCへそのまま準用しない。必須条件は別途確認を要する |
| 3.8.3 pp.227–228（同2–3頁）、解説(2) | 引張柱脚の原則埋込み、切替階で内部鉄骨を応力の小さい位置まで延長、壁縦筋の定着、接合部のせん断と最小帯筋比0.1%=0.001。解説の推奨と政令掲示枠を区別 | `srrc/panel_zone.rs`の耐力比は、埋込み・定着・帯筋配置・切替階の荷重伝達の成立確認を代替しない | **未対応/今回は未確認**。全接合部計算は#886/既存#723等の範囲へ広げないが、別途必要な安全確認は未解決として#697に残す |
| 6.1 pp.337–339（6.01の3–5頁）、令82条・告594第1 | 長短期応力が各許容値以下。設計用部材力と許容耐力の間接比較も可。剛性等は実況に応じ設定。N [N]、M [Nmm]、Q [N] | `SrcDesign::check`→`beam::src_beam_check`/`column::src_column_check`。梁はMz/Qy、柱はN・My/Mz・Qy/Qzを検定 | **限定対応**。梁N/My/Qzの不変結果は安全確認ではない。非ゼロなら必要な検討を別途行う。柱/梁共通化#745はclosed/not_plannedであり実装済みの証拠ではない |
| 9.2 p.566（9.02の1頁）、9.3 p.584（9.03の1頁） | 鋼材長期ft=F/1.5、fs=F/(1.5√3)、短期1.5倍。コンクリート圧縮長期Fc/3・短期2倍 [N/mm²]。指定頁にSRC独自の材料強度割増しなし | `material_strength`、`srrc/beam.rs`/`column.rs`。共通鋼材表は[#879](鉄骨材料と許容応力度_Issue879_原典照合.md)、RCは[#881](RC材料構造細則_Issue881_原典照合.md) | **共通式は既存成果参照**。SRC専用の累加・食込み低減・分担式まで本表で正当化しない。共通材料の残件も解消しない |
| 上記指定節 | 解析ns=Es/Ec、n−1換算、設計用n、fc′=fc(1−15spc)、累加N–M、二軸線形和、せん断分担/併用・pw上限0.006・QDの係数 | [等価断面](../../docs/calc_basis/03_断面性能/06_SRC_CFT_の等価断面性能.md)、[断面検定](../../docs/calc_basis/06_一次設計/04_SRC_CFT_断面検定.md)、`section_shape/composite.rs`/`srrc` | **本書では照合不可（別原典必要）**。今回閲覧節には完全定義がなく、SRC規準1987/2014・RC規準等の採用版・条項・適用範囲の照合が必要。本書全体に存在しないとの断定ではない |

対応実装は[composite.rs](../../crates/sepika-core/src/section_shape/composite.rs)、[srrc/mod.rs](../../crates/sepika-design-jp/src/srrc/mod.rs)、[beam.rs](../../crates/sepika-design-jp/src/srrc/beam.rs)、[column.rs](../../crates/sepika-design-jp/src/srrc/column.rs)、[panel_zone.rs](../../crates/sepika-design-jp/src/srrc/panel_zone.rs)。ソースでの経路確認とGUI通し実行を区別する。

## 独立数値と要素→証拠索引

| 要素 | 独立入力・期待値・境界 | 公開入口・証拠 |
|---|---|---|
| 解析等価A/I/EIと設計nの分離 | b300/D600、H400×200×8×12、フィレット/鉄筋/床なし。Ec20500/Es205000→ns10、Ae250272 mm²、Ie7345337856 mm⁴、EI150579426048000 Nmm²。設計nの27/36/48/60境界と異常材料拒否 | [#434の独立検証](SRC解析用弾性係数比_Issue434_2026-10.md)。閉鎖/PR549の成果を参照し、今回の書籍一致や実験Validationへ格上げしない |
| SRC梁の許容曲げ | 外形300×600、同H、上下各2-D16、かぶり20、帯筋D8。鋭角矩形積分Is216148650.6666667 mm⁴→Z1080743.2533333334 mm³。at=128π=402.1238596594935 mm²、d564/j493.5 mm、長期SN400B ft156.6666666666667、SD345 ft215。採用累加式の独立MA=211982789.84174365 Nmm。0.999999/1/1.000001倍のMzで同検定比 | `SrcDesign::check`、`source884_beam_fixed_allowable_and_boundary`。**採用式の有限Verification**であり、累加式の本書一致ではない |
| 鉄骨かぶり境界 | H幅200、b299.8/300/300.2→横49.9/50/50.1 mm。上下100 mm。法令の原則境界50 mmを跨ぐ | `source884_cover_and_unchecked_components_are_not_qualification`。全てCheckedとなる現行を観測し、法定適合の成功試験としない。例外認定なしを仮定した原則値との比較 |
| 梁の適用制約・未知鋼材 | 基本のMz/Qyを0固定しN=100000000 N、My=100000000000 Nmm、Qz=100000000 Nを単独に与えても検定比0。未知鋼材名では明示fyによらずF235で同じ検定比 | 同公開入口。比0は未評価成分の安全を意味しない。未知fy100/235/345が同じ比1を観測。F100を採用可能とする場合の独立MA114715897.04174364 Nmm・比1.8478937558637216より、低強度未知材で危険側となり得る |

保存した[probe](data/issue884/source_probe.rs)を`crates/sepika-design-jp/tests/source884_probe.rs`へ一時コピーして`cargo test -p sepika-design-jp --locked --test source884_probe -- --nocapture`で実行し、一時コピーだけを削除する。[ログ](data/issue884/probe.log)を保持する。期待値の生成に製品helperを使わない。共通材料の全種類、柱N–M全分岐/全軸、QD全組合せの再検証ではない。既存#671のQL/必要参照の検証も保持する。

## 見送りと継続確認

- **今回対象外**: SRC終局強度・靭性は#886、耐震ルートは#885、CFTは対象外。共通鋼材/RC材料は#879（PR903）/#881（PR900）の成果と#697の残件を参照し、再調査しない。
- **本書では照合不可（別原典必要）**: 上表の換算・許容累加/分担式の完全な根拠。#434はSRC2014原本・全ねじり/せん断・Fc>60を未確認としており、それを維持する。方式差や指定頁未掲載だけで改式必須としない。
- **今回は未確認**: 付録1-4を含む全SRC設計例、全複合断面形状、施工細則の個別除外認定、付着滑り・局部座屈・実建物/実験Validation。有限重点外であり、適用可能と判定したわけではない。
- **必須条件/安全性の継続**: かぶり・準用条件・柱脚/定着/切替階の成立確認を部材比だけから推定できない。[#697への追記](https://github.com/hrntsm/SEPIKA/issues/697#issuecomment-6422778289)で継続する。梁N/My/Qzが非ゼロなら未評価を適合と誤読すると危険側になり得る。#745のnot_planned閉は安全性の解決でも共通化実装済みでもなく、再開や重複起票はしない。#697で個別建物の適用条件と別途安全確認を継続する。

未知鋼材名のF235補完は、適用可能な明示強度を無視して低強度材の耐力を過大にし得る具体差異であり、[材料解決の修正課題#912](https://github.com/hrntsm/SEPIKA/issues/912)へ接続する。単なる方式差・本書未掲載とは区別し、本調査で計算実装は変更しない。製品docsの無条件な原典適合表現を限定し、未知鋼材補完の危険側可能性を明記する。

## 実行結果

- 一時probe初回はtest-onlyの`unwrap_checked`をintegrationから呼んだコンパイル失敗。公開enumのmatchへ直した。証拠は[初回ソース](data/issue884/source_probe_initial.rs)・[ログ](data/issue884/probe_initial.log)。
- 2回目は1成功・1失敗。未知材のSkippedというソース読みの推測を実行結果が否定した。[2回目ソース](data/issue884/source_probe_second.rs)・[ログ](data/issue884/probe_second.log)を保持。製品修正成功と混同しない。
- 最終probeは2成功・0失敗。MA3点、かぶり3点、梁未評価成分単独3点、未知fy3点を観測し、一時integrationファイルは削除した。
- `cargo test -p sepika-core --locked src_analysis_ratio_matches_independent_rectangular_h_section`: 1成功。[ログ](data/issue884/core.log)。
- `cargo test -p sepika-design-jp --locked allowable_bending_ratio_boundaries_are_separate_from_analysis_ratio`: 1成功。[ログ](data/issue884/n_boundary.log)。
- `cargo run -p xtask -- check-docs`: 成功（summary124、links656、impl refs705）。`mdbook build`: 成功。
- `git diff --check`: 成功。変更した開発文書のローカルリンク・実装参照先を確認。
- 保存probeは公開design入口のVerificationで、実験ValidationやGUI編集・解析・保存の新規通し試験ではない。
## 先行RC照合との統合

2026-10-27に`origin/main`の`35bd8d5107cd32b78466f6fc544f6bf183a8d94c`（PR #911）を統合した。先頭の対象hashと保存probeは実行時点の証拠として維持する。README・未検証一覧の同時追記は双方を保持し、原典照合リスト・RC製品docs・#883の独立証拠も保持した。

`7635aeed`から統合先までの`crates/sepika-design-jp/src/srrc/`、`crates/sepika-core/src/section_shape/composite.rs`、`crates/sepika-design-jp/src/material_strength/`に差分はなく、今回の公開検定と等価断面の観測へ影響する変更はない。計算コード不変のためprobeの再実行は行っていない。

統合後の`cargo run -p xtask -- check-docs`は成功（summary124、links657、impl refs704）、`mdbook build`も成功した。`git diff origin/main --check`は成功し、#884の変更文書のローカル参照先を再確認した。統合時のstaged全差分検査には、先行#883の`probe.log`・`probe_main_followup.log`の既存末尾空行が各1件表示された。先行実行ログの内容は変更せず保持する。実験Validation等の未確認範囲と#912/#697の追跡は変わらない。
