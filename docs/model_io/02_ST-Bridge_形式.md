# ST-Bridge 形式（.stb / .xml）

[ST-Bridge](https://www.building-smart.or.jp/meeting/buildingsmart/st-bridge/)（XML, 2.0 系）で**読み込み・書き出し**ができ、他社の一貫計算プログラムや BIM ツールとモデルを受け渡すための入出力経路として機能します。

<div class="impl-ref">

**実装参照**：`sepika_io::stbridge::{import_stbridge_with_report, export_stbridge}`（`crates/sepika-io/src/stbridge/`）が取り込み・書き出しの入口です。

</div>

## GUI からの操作

| メニュー | 動作 |
|---|---|
| 📥 **ST-Bridge 読込…** | `.stb`（または `.xml`）ファイルを選び、内部モデルへ取り込みます。取り込んだモデルは検証（`validate`）を通ってから現在のモデルと差し替わります |
| 📤 **ST-Bridge 書出…** | 標準 ST-Bridge 2.0.2（`StbSecColumn_S` 等＋形鋼ライブラリ）で書き出す。BIM・他ソフト向け |

- ファイル選択ダイアログの拡張子フィルタは `.stb` / `.xml`。
- ST-Bridge 読込は `.ovika` プロジェクトとは別系統であり、読み込んでもプロジェクトの保存先パスは設定されません（新規モデルとして開く扱い）。保存するとネイティブの `.ovika` として保存されます。
- 書き出しは ST-Bridge 2.0.2 の標準要素を用いる1形式です。全断面・部材の標準適合は未完了で、RC梁の配筋入力不足と一部ブレースの出力には制約があります。受け渡し先での検証を確認し、完全一致の保存にはネイティブの `.ovika` を使います。

<div class="impl-ref">

**実装参照**：メニューは `sepika_app`（`crates/sepika-app/src/app/mod.rs`）が表示し、取り込みは `sepika_app::app::App::import_stbridge_from`、書き出しは `sepika_app::app::App::export_stbridge_to`（`crates/sepika-app/src/app/actions/io.rs`）が担います。

</div>

## 対応バージョン

- **ST-Bridge 2.0 系のみ**を受け付けます（ルート要素 `ST_BRIDGE` の `version` 属性が `2.` で始まること）。
- 1.x 系や ST-Bridge でない XML は読み込みエラーになります。
- ファイルの文字コードは、BOM 付き UTF-8・UTF-8・Shift_JIS（Windows-31J）の順に判定します。

<div class="impl-ref">

**実装参照**：文字コードは `sepika_io::stbridge::read_stbridge_file`（`crates/sepika-io/src/stbridge/import/mod.rs`）が判定し、`version` 属性の検証は `sepika_io::stbridge::import::parser::parse`（`crates/sepika-io/src/stbridge/import/parser.rs`）が行います。

</div>

## 対応範囲（意味的往復を保証するサブセット）

原階の元 ID/GUID・名称・height・kind・id_dependence・strength_concrete・明示節点列を保存し、
節点の元 ID/GUID も再出力します。内部の連番 ID は交換形式の ID とは別に扱います。
新規の階・構造節点は編集時に既存と衝突しない正整数 ID を保持し、並べ替えや undo/redo で変わりません。
標準節点 ID がまだ無いネイティブモデルでは、最初の成功編集時に既存構造節点の ID も保存します。
初回の削除でも残存節点の標準 ID を保持し、拒否された編集は ID を追加しません。undo は編集前の保存状態へ戻します。
未保存の既存階も、階追加・変更・削除の前に標準 ID を保存するため、中間階を削除しても残存階の標準 ID は変わりません。
原階が未初期化のネイティブモデルでは、節点側と階側の所属指定を合わせた、その時点の標準出力所属を初回階編集でも保持します。
同じ階への重複はまとめ、二つの階への所属指定が競合する場合は両方を保持して多重所属を診断します。
準備計算で推定した解析所属や生成した剛床代表節点を、元の明示所属へ合算しません。

コンクリートの強度指定と省略状態は、部材・断面・原階・共通ごとに保持します。
採用値を元属性へ補完して保存しないため、再出力で省略の優先順位が変わりません。

## 標準強度の解決と編集

コンクリートのFcは **部材 → 断面 → 指定節点の明示原階 → 共通** の順に採用します。
上位属性が存在しても未知名・空文字・非正値・非有限値なら未解決とし、下位の既知値へ迂回しません。
全指定の欠落も未解決です。原階の参照は次の節点で決まり、近傍階・平均標高・準備計算の解析所属では代用しません。

| 部材 | 材料参照節点 |
|---|---|
| 柱・間柱 | `id_node_top` |
| 大梁・小梁 | `id_node_start` |
| 床 | 元 `StbNodeIdOrder` の第1節点 |
| 壁 | 元 `StbNodeIdOrder` の最終節点 |

原階に異なるFcが競合する場合、上位の部材・断面で確定していれば材料は解決できます。
材料の採用元と、原階の所属・依存関係の診断は別に確認します。

鉄筋は、各部位の `strength_*` があればその指定を採用し、省略時だけ対応する `D_*` と
共通の `StbReinforcementStrengthList` の `D`/`strength` を照合します。
主筋・副主筋・帯筋・あばら筋・腹筋・巾止筋、位置別指定をそれぞれ保持します。
径や強度の欠落をD13・SD345で補完しません。同径の競合と未知名は理由を示して解析を停止します。
位置別の各指定が解けても、現行計算で一つの主筋・せん断補強筋・内蔵鋼材へ縮約できない強度は解析に採用できません。
高強度鉄筋のgrade読取りと、各検定式の適用範囲は別です。

`StbCommon` の `project_name`・`app_name`・`app_version`、任意の `strength_concrete` と径別リストを保存します。
共通子要素の出力順は径別リスト、適用条件リストの順です。配筋全体と `ApplyConditionsList` の写像は未対応です。
説明図中の `concrete_strength` は標準属性 `strength_concrete` の別名として受け付けません。

準備計算の「STB強度指定と採用元」で、元指定の有無、共通・原階・部材・断面・鉄筋部位の指定と採用結果を確認・編集します。
チェックを外すと元属性を省略し、チェックを入れた空欄は明示された空文字として未解決になります。
通常の材料テーブルの名称変更は表示名の変更であり、保存された標準gradeやFc・Fyを変更しません。
Fc数値の意図的な変更は、その材料を採る元明示指定へ反映します。E・ν・密度の編集では元強度指定・省略を変えません。
断面への明示材料割当は、その断面の強度を変更します。主材料・主筋・せん断補強筋・SRC内蔵鉄骨を別々に指定できます。S断面の主材料は鋼材、RC・SRC・CFT断面の主材料はコンクリートです。鋼材割当を解除した場合は、必須の鋼材強度が未解決になり、旧gradeへ戻りません。上位の部材Fc指定があれば部材の採用値はそのままです。
標準鋼材gradeの編集は、Sの主材料・二次部材の鋼材・SRC内蔵鋼の実採用材料にも反映します。明示材料割当がある場合は、その材料の数値を採用し、標準gradeによる採用と区別します。明示割当だけの操作で別の既定材料を追加しません。
鉄筋や鋼材のFy数値を編集した場合は、その数値を採用し、元の標準gradeとは区別します。標準gradeで編集後の数値を表せない場合はSTB出力を理由付きで拒否します。独自の標準gradeを生成せず、任意物性の保存にはOVIKAを使います。
明示材料の物性を採る結果には「明示材料指定」を表示し、標準gradeからの解決と区別します。
任意の明示鉄筋物性と標準gradeの数値が一致しない場合は、ネイティブ保存と標準書出しを区別し、標準で表せない出力を拒否します。

ST-Bridge強度名の解決とE・ν・密度の既定復元は別の処理です。
認識済みgradeの強度を採用した後、明示物性を持たない材料にSEPIKAの[標準材料既定値](../calc_basis/02_材料/04_標準材料一覧.md)を与えます。
ST-Bridgeに存在しない独自材料表は追加しません。任意物性を含む完全保存には `.ovika` を使います。
長さmm・角度deg・強度N/mm²を保持します。

### 強度の入出力制限

| 条件 | 読取り・材料供給 | 標準書出し |
|---|---|---|
| 対応部位の強度・径・省略 | 元指定と採用元を保持 | 元属性を再出力。解決値を各部材へ補完しない |
| 床が領域ごとに分割される | 各分割床へ元第1節点からFcを供給 | 元第1節点を境界で表現できず原階・共通依存となる省略は、理由付きで拒否。明示部材・断面Fcで意味を保持できる場合は区別する |
| 同領域へ複数の元床を合算する | 元の強度・採用元が異なる場合は拒否 | 任意の先勝ち値で出力しない |
| 位置別配筋を現行出力で表せない | 各部位の元指定を保持 | 理由付きで拒否。配筋全写像は未対応 |
| 断面系列間で同じ数値IDを使う | 現行参照で区別できず理由付きで拒否 | [型別ID対応の課題](https://github.com/hrntsm/SEPIKA/issues/571) |

床の元幾何を再結合して出力する機能はありません。取り付く壁版や配筋全体にも既存の出力制限があります。
材料を対象にしたXMLの適合が、建物全体の標準適合を保証するわけではありません。

出典: buildingSMART Japan「ST-Bridge XML仕様説明書 ver.2.0.2」2022-06-15、§2.4（本文11頁）、
§3.2–3.2.4（本文18–24頁、優先順20頁）、§4.4.1（本文57–58頁）、§6.2.5（補足4頁）。
[採用対象の仕様説明書](https://www.building-smart.or.jp/wp-content/uploads/2022/06/ST-Bridge_XML%E4%BB%95%E6%A7%98%E8%AA%AC%E6%98%8E%E6%9B%B8%EF%BC%88ver.2.0.2%EF%BC%89_20220615.pdf)、
[公式配布入口](https://www.building-smart.or.jp/meeting/buildingsmart/st-bridge/)。標準属性・子要素順序は2.0.2 XSDによります。

<div class="impl-ref">

**実装参照**：`sepika_core::model::Model::{resolve_stb_concrete, resolve_stb_rebar, resolve_stb_steel}`（`crates/sepika-core/src/model/strength.rs`）と `sepika_io::stbridge::strength_export`（`crates/sepika-io/src/stbridge/strength_export.rs`）が解決と元指定の出力を担います。

</div>

読み込み・書き出しの対象は、`import → export → 再 import` でモデルが**意味的に一致する**範囲に限定しています。

要素ごとの詳細な変換状況（取り込み／書き出し／往復・備考）は、
[ST-Bridge 要素別 変換状況一覧](./03_ST-Bridge_要素別変換状況.md)を参照してください。

<div class="impl-ref">

**実装参照**：取り込みと書き出しは `sepika_io::stbridge::{import, export}`（`crates/sepika-io/src/stbridge/`）が担い、床板・壁版の床領域／壁領域への付け直しは `sepika_core::{region_rebuild, wall_region_rebuild}` が行います。

</div>

## 非対応（対象外）

非対応項目は入出力の対象外です。取り込み時の扱いは、[ST-Bridge 要素別変換状況一覧](./03_ST-Bridge_要素別変換状況.md)を参照してください。

- 剛域・製作情報などの詳細属性。
- 解析結果・SEPIKA 独自の解析／設計属性。ST-Bridge の対象外で、保持には [`.ovika`](./01_プロジェクト形式_ovika.md) を使います。

未対応の要素（`StbFooting`・`StbPile` など）を含む、取り込み時にデータを欠落させる要素は、取りこぼしを無言で捨てず `ImportReport` の `warnings`
（Rust API では `report.warnings`）として通知します。手動リストにない新要素・ベンダー拡張も、部材グループ・
断面の直属子であれば要素名で通知します。GUI の「ST-Bridge 読込」は警告があれば「⚠️ 取り込み時の注意」として表示します。

<div class="impl-ref">

**実装参照**：未対応要素の収集と警告への変換は `sepika_io::stbridge::import`（`crates/sepika-io/src/stbridge/import/{parser.rs, assemble.rs}`）が担い、`sepika_io::stbridge::ImportReport` として返ります。

</div>

### 属性の扱いの報告

要素の警告とは別に、**属性の単位でも、ファイルに存在したものをどう扱ったかをすべて報告します**。
取り込みの報告 `ImportReport.attributes` に、要素名・属性名ごとの出現件数と、そのうち
取り込んだ件数を持ちます。1 件も取り込まなかった属性は `report.dropped_attributes()` で取得できます。
属性の未取り込みは `warnings` には積まず、`attributes` とログで報告します。

無視リストは持ちません。
節点・原階の GUID と共通のアプリ情報は保存し、それ以外の未対応の `guid` は「取り込まなかった属性」として現れますが、
どの属性がどう扱われたかを利用者が漏れなく追えることを優先しています。
リストを持たない副次的な利点として、取り込みの実装を増やせば報告も自動的に追随するため、
一覧の更新漏れが起きません。

GUI の「ST-Bridge 読込」では、扱いの全量を下ドックの「ログ」タブへ出します。
読込直後のメッセージには「取り込まなかった属性が N 種類あります」という要約だけを出します。
実ファイルでは属性の種類が数十になり、そのまま並べると読めなくなるためです。

同じ属性でも文脈により扱いが分かれることがあります。
たとえば 1 つの断面に図形要素が複数ある場合、最初の図形だけを採るため 2 つ目以降の寸法属性は
参照しません。
このときログには「N 件中 M 件を取り込み」と出ます。

<div class="impl-ref">

**実装参照**：属性の出現・取り込み件数は `sepika_io::stbridge::AttrDisposition` として集計し（`crates/sepika-io/src/stbridge/import/{parser.rs, assemble.rs}`）、GUI のログ出力は `sepika_app::app::App::log_attribute_dispositions`（`crates/sepika-app/src/app/actions/io.rs`）が担います。

</div>

### 支点の自動設定（取り込み時）

ST-Bridge は境界条件（支点）を持たないため、支点が 1 つもないモデルを取り込んだ
ときは、そのままでは解析できません。そこで取り込み時に、**最下レベル（Z 最小、
許容差 1 mm）で柱脚が取り付く節点**をピン支点（並進 3 自由度を拘束・回転自由）に
自動設定します（柱脚ピンの仮定。基礎の回転拘束を期待しない安全側の既定）。この支点自動設定に限り、柱脚候補を鉛直な 2 節点線材（部材軸の鉛直成分が大きい部材）で判定し、
柱が取り付かず梁だけが取り付く最下レベル節点（地中梁の中間節点など）は支点にしません。
最下レベルに柱脚が特定できない場合に限り、解析可能性を優先して最下レベルの全節点を
ピン支点にフォールバックします。設定した内容は `ImportReport` の通知（notes）で知らせ、
モデルタブ→境界条件で変更できます。すでに拘束を持つモデルはそのまま尊重し、何もしません。

<div class="impl-ref">

**実装参照**：`sepika_io::stbridge::import::assemble::auto_assign_supports`（`crates/sepika-io/src/stbridge/import/assemble.rs`）が判定・設定し、通知は `sepika_io::stbridge::ImportReport` の notes に積みます。

</div>

## 断面用途の取り込み

主架構線材の設計用途は、断面に保持する `FrameSectionUse`（`Girder`・`Column`・`Brace`）を正とします。
ST-Bridge の `StbColumn`・`StbGirder`・`StbBrace` の部材コンテナから用途を割り当て、部材の角度・鉛直性・断面符号から柱・梁・ブレースを推定しません。
したがって、部材の角度にかかわらず `StbGirder` は大梁として扱います。

`ElementKind` は解析定式化、`FrameSectionUse` は設計用途を表します。主架構の `ElementKind::Beam` には `Girder` または `Column`、
`ElementKind::Brace` には `Brace` を割り当てます。不整合または用途不明の断面は、角度で補正せず取り込みをエラーにします。
二次部材は `SecondaryMemberKind`（小梁 `Beam`・間柱 `Post`）を正とし、二次部材から `FrameSectionUse` は決めません。小梁・間柱が参照する断面は `frame_use=None` でも構わず、同一断面を主架構と共有する場合を含め、`frame_use` の値だけを理由に二次部材を拒否しません。

<div class="impl-ref">

**実装参照**：ST-Bridge の部材コンテナから用途を解決する処理は `sepika_io::stbridge::import::assemble::section_uses`（`crates/sepika-io/src/stbridge/import/assemble.rs`）が担い、要素種別と断面用途の整合性は `sepika_core::model::Model::validate`（`crates/sepika-core/src/model/aggregate.rs`）が検証します。

</div>

## 断面の符号と階（取り込み）

ST-Bridge は断面を `guid` で識別しており、**同じ符号の断面を階ごとに別の定義として持ちます**。
そのため 1 つのファイルに `name="C1"` の `StbSecColumn_S` が、`floor="1"`・`floor="2"`・`floor="3"`
と 3 つ並ぶことは正常な状態です。

SEPIKA の断面は[符号と階の組](../model_edit/02_断面の符号と階.md)で識別するため、この構造を
そのまま取り込めます。
断面の `floor` 属性は文字列としてそのまま保持し、階（`StbStory`）への参照としては扱いません。
ST-Bridge の `floor` は自由文字列で、階への id 参照を持たないためです。
実際、断面の階名が `1`・`R`・`PHR` で `StbStory` の名称が `1FL`・`RFL`・`PHRFL` というように
食い違うファイルや、断面の階名に対応する `StbStory` が存在しないファイルがあります。

符号＋階が重複する断面定義は、取り込みの段階で次のように解決します。

- 断面性能・断面形状・**材料**が完全に一致する定義は、1 件へ統合する。統合した件数は
  `ImportReport` の通知（notes）で知らせる
- いずれかが食い違う定義は、符号へ連番を付けて（`b3` → `b3#2`）別の断面として残す。
  符号を変えた内容は `ImportReport` の警告で知らせる

材料も一致の判定に含めるのは、材料は断面が持ち、違う材料を割り当てるならそれは
別の断面だからです。材料だけが違う定義を 1 件へまとめると、片方の材料が無言で消えます。

食い違う定義を捨てずに残すのは、利用者が書いた断面定義を無言で消さないためです。
連番を付けた時点で符号＋階は一意になるので、モデル側の制約とも矛盾しません。

階を持たない断面定義（`floor` 属性のない `StbSecBeam_S` など）が複数あり、内容も同一である場合は
1 件へ統合されます。
小梁の断面定義が同じ内容で何件も並ぶファイルは実際にあるため、この統合で断面一覧が実態に近づきます。

<div class="impl-ref">

**実装参照**：重複の統合・連番付与・材料を含む一致判定は `sepika_io::stbridge::import::assemble::build_sections`（`crates/sepika-io/src/stbridge/import/assemble.rs`）が担います。

</div>

## 断面表現（書き出し）

書き出しは **ST-Bridge 2.0.2 の標準要素を用いる** 1 形式です。全体のスキーマ適合は未完了で、
対応範囲と制限は上記の対応範囲・非対応項目を参照してください。
内部モデルのパラメトリック断面形状（`Section.shape`）を対応する標準要素＋形鋼ライブラリ（`StbSecSteel`）へ写像します。

断面ごとの書き出し先と、取り込み・書き出し・往復の詳細は、[ST-Bridge 要素別 変換状況一覧](./03_ST-Bridge_要素別変換状況.md)を参照してください。

材料の物性値まで含めた完全一致での往復が必要な場合は
[`.ovika`](./01_プロジェクト形式_ovika.md) を使います。

<div class="impl-ref">

**実装参照**：断面の写像は `sepika_io::stbridge::section_std`（`crates/sepika-io/src/stbridge/section_std.rs`）が、XML 全体の組み立ては `sepika_io::stbridge::export`（`crates/sepika-io/src/stbridge/export.rs`）が担います。

</div>

## 開発者向け: ライブラリ API（Rust）

<details>
<summary>Rust コード例（開発者向け）</summary>

```rust
use sepika_io::stbridge::{import_stbridge, import_stbridge_with_report, export_stbridge};

// 読み込み: ST-Bridge XML 文字列 → 内部モデル
let xml = std::fs::read_to_string("model.stb")?;
let model = import_stbridge(&xml)?;

// 読み込み（欠落の報告つき）: 未対応要素のスキップ・断面欠落・参照解決失敗などを警告として得る
let (model, report) = import_stbridge_with_report(&xml)?;
if !report.is_clean() {
    for w in &report.warnings {
        eprintln!("警告: {w}");
    }
}

// 書き出し: 内部モデル → 標準 ST-Bridge 2.0.2 XML 文字列
let xml = export_stbridge(&model)?;
std::fs::write("model.stb", xml)?;
```

</details>
