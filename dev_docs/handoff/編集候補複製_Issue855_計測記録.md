作成日: 2026-10-26

# 編集候補のモデル全体複製回数（Issue #855）

## 計測対象と結果

Windows / Rust 1.99.1、`cargo test -p sepika-edit --locked` の test profile で、
`Model::clone` 自体への一時ログを数えた。速度・メモリ使用量は計測していない。
モデルは X 方向に 1 m 間隔の節点3個と梁2本、材料・断面・版・階は未設定。
単一編集は節点1を X=1100 mm へ、複合編集は節点1を同位置へ、節点2を X=2200 mm へ変更する。
各操作の間に計測ラベルを出力し、その区間の `MODEL_CLONE` 行数を集計する。

| 操作 | 変更前 | 変更後 |
| --- | ---: | ---: |
| 単一編集 run | 2 | 1 |
| 単一編集 undo | 0 | 0 |
| 単一編集 redo | 1 | 1 |
| 複合編集 preview | 3 | 1 |
| 複合編集 run | 3 | 1 |
| 複合編集 undo | 2 | 1 |
| 複合編集 redo | 3 | 1 |
| 非境界編集 preview | 0 | 0 |

変更前は `b8d61a04`、変更後は同コミット上の Issue #855 実装で計測した。
その後、階レベル編集の変更を含む `96b182b1` へ rebase し、下記の再現手順を実行して変更後の全回数が同じであることを確認した。この計測モデルは階を持たない。
単一座標編集の逆操作は断片スナップショットを直接復元するため undo は0回、redoは候補保持の1回となる。
複合 undo は `UndoStack::undo` が候補を持たないため、直接 `CompositeCommand::apply` の1回が必要になる。

## 再現手順

対象コミットを別作業ディレクトリへ用意し、未コミット変更がない状態で以下を
`measure_issue855.py` としてルートへ一時保存し、`python measure_issue855.py` で実行する。
スクリプトは Model の自動 Clone を全フィールドの同等な手動 Clone とログへ一時置換し、
既存 `seeded_model` を使う計測テストを追加する。終了時には正常・失敗にかかわらず両ファイルを元へ戻す。
実行後はスクリプトを除去し、`git diff` で一時計装が残っていないことを確認する。
Cargo を並行実行せず、計測中に対象ソースを編集しない。

```python
from pathlib import Path
import re
import subprocess

aggregate = Path("crates/sepika-core/src/model/aggregate.rs")
tests = Path("crates/sepika-edit/src/tests.rs")
original_aggregate = aggregate.read_bytes()
original_tests = tests.read_bytes()
try:
    source = original_aggregate.decode("utf-8").replace("\r\n", "\n")
    source = source.replace(
        "#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]\npub struct Model",
        "#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]\npub struct Model",
    )
    fields = re.findall(r"^    pub (\w+):", source.split("pub struct Model {")[1].split("\n}")[0], re.M)
    source += '\nimpl Clone for Model {\n    fn clone(&self) -> Self {\n        eprintln!("MODEL_CLONE");\n        Self {\n'
    source += "".join(f"            {name}: self.{name}.clone(),\n" for name in fields)
    source += "        }\n    }\n}\n"
    aggregate.write_text(source, encoding="utf-8")
    tests.write_text(original_tests.decode("utf-8") + r"""
#[test]
fn measure_model_clones_issue855() {
    let mut model = seeded_model(3, 2);
    let mut stack = UndoStack::new();
    fn command() -> Box<dyn EditCommand> {
        Box::new(SetNodeCoord { node: NodeId(1), coord: [1100.0, 0.0, 0.0] })
    }
    fn composite() -> Box<dyn EditCommand> {
        Box::new(CompositeCommand { label: "計測".into(), children: vec![command(), Box::new(SetNodeCoord { node: NodeId(2), coord: [2200.0, 0.0, 0.0] })] })
    }
    eprintln!("CASE single run");
    assert!(stack.run(&mut model, command()));
    eprintln!("CASE single undo");
    stack.undo(&mut model);
    eprintln!("CASE single redo");
    stack.redo(&mut model);
    eprintln!("CASE composite preview");
    preview_plate_assignment_loss(&model, composite().as_ref()).unwrap();
    eprintln!("CASE composite run");
    assert!(stack.run(&mut model, composite()));
    eprintln!("CASE composite undo");
    stack.undo(&mut model);
    eprintln!("CASE composite redo");
    stack.redo(&mut model);
    eprintln!("CASE nonboundary preview");
    preview_plate_assignment_loss(&model, &SetNodeRestraint { node: NodeId(0), restraint: Dof6Mask::FIXED }).unwrap();
    eprintln!("CASE end");
}
""", encoding="utf-8")
    subprocess.run([
        "cargo", "test", "-p", "sepika-edit", "--locked", "measure_model_clones_issue855",
        "--", "--nocapture", "--test-threads=1",
    ], check=True)
finally:
    aggregate.write_bytes(original_aggregate)
    tests.write_bytes(original_tests)
```

## 採否判断

原子性の候補を追加せず既存境界で共有する方針は [ADR 0042](../adr/0042-attached-slab-validation-and-support-resolution.md) に追記した。
境界変更の有無だけで複合編集の最終検証を飛ばす案は、公開 trait の異常入力に対する拒否順を変えるため採用しなかった。
同一状態の検査削減は、非境界編集の事前表示から変更前の領域識別検査を除く範囲とした。
計測回数は当該経路の証拠であり、モデル規模ごとの速度改善量を示すものではない。
