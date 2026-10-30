# ST-Bridgeの内部表現とAPI

利用者向けの対応範囲・既定値・確認操作の正本は[ST-Bridge形式](../docs/model_io/02_ST-Bridge_形式.md)と[要素別変換状況](../docs/model_io/03_ST-Bridge_要素別変換状況.md)です。本書は内部構造と開発時の調査入口を扱います。

## 用途と解析定式化

主架構の `FrameSectionUse`（`Girder`・`Column`・`Brace`）は設計用途、`ElementKind` は解析定式化です。`ElementKind::Beam` は柱・大梁の双方を含みます。二次部材は `SecondaryMemberKind`（小梁 `Beam`・間柱 `Post`）で区別し、`frame_use=None` も許容します。

調査入口は `crates/sepika-io/src/stbridge/import/parser.rs` の `start_member`、`assemble.rs` の `section_uses`、`crates/sepika-core/src/model/aggregate.rs` の `Model::validate` です。支点の幾何判定は `assemble.rs` の `auto_assign_supports` と `crates/sepika-core/src/geom.rs` の `is_vertical_axis` に分離されています。

## 識別と再採番の調査入口

入力の断面参照は `SectionKey`（具体系列と元数値ID）から内部断面IDへ対応付けます。`assemble.rs` の `build_index` はファイルIDを昇順の連番へ写像します。出力は `section_std.rs` の断面ID写像を使い、原階・構造節点の標準ID/GUIDとは分けて扱います。

原階・構造節点の初回編集時のID確保とUndoは `crates/sepika-core/src/model/`、標準強度の参照と再採番は同ディレクトリの `strength.rs`、出力は `crates/sepika-io/src/stbridge/strength_export.rs` を参照します。

## 属性報告の内部契約

`ImportReport.attributes` は `AttrDisposition` の要素名・属性名ごとの出現件数と取込件数を持ちます。`dropped_attributes()` は取込件数0の属性を返し、属性の未取り込みは `warnings` ではなく属性集計とログへ出します。パーサの属性参照に連動して記録するため、無視リストは設けません。

GUIの出力入口は `crates/sepika-app/src/app/actions/io.rs` の `log_attribute_dispositions` です。要素の欠落等は `report.warnings`、支点自動設定等の通知は `report.notes` に分けます。

## XML字句と配筋の検証

鋼ブレースの要素間内容はXML空白（空白・タブ・CR・LF）のみに制限し、名称中の `&`・引用符はエスケープ、バックスラッシュは保存します。根拠は[公式2.0.2 XSD](https://www.building-smart.or.jp/wp-content/uploads/2026/04/ST-Bridge_v202.zip)の同名複合型と[W3C XML Schema Part 1 §3.4.4](https://www.w3.org/TR/xmlschema-1/#cvc-complex-type)のelement-only契約です。

配筋の数値属性とApply/isSpiralのbooleanはXML Schemaのcollapseに従ってXML空白を解釈し、元字句は別に保存します。enum/stringには数値用の正規化を適用しません。SRC子要素順序、鋼ブレースのelement-only内容と名称エスケープは、全文の標準適合と分けた限定モデルの検証です。

調査用テストは `crates/sepika-io/tests/stbridge_standard_rebar.rs`、`stbridge_standard_input.rs`、`stbridge_section_series.rs`、`stbridge_built_box.rs`、`stbridge_brace_content.rs` です。MCPでの配筋通知は `model.query` の `rebar_notices`（`blocking: false`）を参照します。

## ライブラリAPI（Rust）

<details>
<summary>Rust コード例（開発者向け）</summary>

```rust
use sepika_io::stbridge::{import_stbridge, import_stbridge_with_report, export_stbridge};

let xml = std::fs::read_to_string("model.stb")?;
let model = import_stbridge(&xml)?;

let (model, report) = import_stbridge_with_report(&xml)?;
if !report.is_clean() {
    for w in &report.warnings {
        eprintln!("警告: {w}");
    }
}

let xml = export_stbridge(&model)?;
std::fs::write("model.stb", xml)?;
```

</details>
