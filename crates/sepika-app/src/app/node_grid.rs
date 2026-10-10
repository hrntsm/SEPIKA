//! 節点テーブル（座標 3 列）用のグリッドアダプタ（グリッド操作）。
//!
//! egui 非依存で、ヘッドレステスト（app/tests.rs）からモデル・undo と
//! 組み合わせて検証できる。モデル編集はすべて sepika-edit のコマンド
//! （複数変更は CompositeCommand 1 個）として UndoStack 経由で行い、
//! ペースト・クリア・行削除が undo 1 回で丸ごと戻る。

use std::collections::BTreeMap;

use crate::grid::GridAdapter;
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::NodeId;
use sepika_core::model::Model;
use sepika_edit::{AddNode, CompositeCommand, DeleteNode, EditCommand, SetNodeCoord, UndoStack};

/// 節点テーブルの [`GridAdapter`] 実装。テーブル描画のフレーム中だけ
/// モデルと undo スタックを借用する使い捨て構造体。
pub struct NodeGridAdapter<'a> {
    pub model: &'a mut Model,
    pub undo: &'a mut UndoStack,
    /// この借用中にモデルを変更したか（呼び出し元が staleness.mark_edited する）
    pub edited: bool,
}

impl NodeGridAdapter<'_> {
    fn run(&mut self, cmd: Box<dyn EditCommand>) {
        self.edited |= self.undo.run(self.model, cmd);
    }

    /// 0 個 = 何もしない、1 個 = 単独コマンド（固有の undo ラベルを保つ）、
    /// 複数 = CompositeCommand 1 個（undo 1 回で丸ごと戻す）として実行する
    fn run_all(&mut self, mut children: Vec<Box<dyn EditCommand>>, label: &str) {
        match children.len() {
            0 => {}
            1 => {
                let cmd = children.pop().expect("len==1 を確認済み");
                self.run(cmd);
            }
            _ => self.run(Box::new(CompositeCommand {
                label: label.to_string(),
                children,
            })),
        }
    }

    /// 既存行のセル変更を「1 行 1 SetNodeCoord」へまとめる
    /// （現座標に変更列を重ねた座標で置き換える）
    fn coord_updates(&self, cells: &[(usize, usize, f64)]) -> BTreeMap<usize, [f64; 3]> {
        let mut updates: BTreeMap<usize, [f64; 3]> = BTreeMap::new();
        for (r, c, v) in cells {
            if let Some(node) = self.model.nodes.get(*r) {
                updates.entry(*r).or_insert(node.coord)[*c] = *v;
            }
        }
        updates
    }
}

impl GridAdapter for NodeGridAdapter<'_> {
    fn rows(&self) -> usize {
        self.model.nodes.len()
    }

    fn cols(&self) -> usize {
        3
    }

    fn row_label(&self, row: usize) -> String {
        row.to_string()
    }

    fn cell_text(&self, row: usize, col: usize) -> String {
        self.model
            .nodes
            .get(row)
            .map(|n| format!("{}", n.coord[col]))
            .unwrap_or_default()
    }

    fn validate_cell(&self, row: usize, col: usize, text: &str) -> Result<(), String> {
        if col >= 3 {
            return Err(format!("節点ID {row}: 座標列の範囲外"));
        }
        match text.parse::<f64>() {
            Ok(value) if value.is_finite() => Ok(()),
            _ => Err(format!("節点ID {row}: 有限の数値を入力してください")),
        }
    }

    fn apply_block(
        &mut self,
        cells: &[(usize, usize, String)],
        append_rows: usize,
    ) -> Result<bool, String> {
        if cells.is_empty() {
            return Ok(false);
        }
        for (row, col, text) in cells {
            self.validate_cell(*row, *col, text)?;
            if *row >= self.rows().saturating_add(append_rows) {
                return Err(format!("節点ID {row}: 追加行の範囲外"));
            }
        }
        let revision = self.undo.revision();
        let n0 = self.model.nodes.len();
        let parsed: Vec<(usize, usize, f64)> = cells
            .iter()
            .filter_map(|(r, c, t)| t.parse::<f64>().ok().map(|v| (*r, *c, v)))
            .collect();
        let existing: Vec<_> = parsed.iter().filter(|(r, _, _)| *r < n0).copied().collect();
        let updates = self.coord_updates(&existing);
        let mut added = vec![[0.0f64; 3]; append_rows];
        for (r, c, v) in parsed.iter().filter(|(r, _, _)| *r >= n0) {
            if let Some(coord) = added.get_mut(r - n0) {
                coord[*c] = *v;
            }
        }
        let mut children: Vec<Box<dyn EditCommand>> = Vec::new();
        for (row, coord) in &updates {
            children.push(Box::new(SetNodeCoord {
                node: NodeId(*row as u32),
                coord: *coord,
            }));
        }
        for coord in added {
            children.push(Box::new(AddNode {
                coord,
                restraint: Dof6Mask::FREE,
            }));
        }
        self.run_all(children, "節点座標の貼り付け");
        if let Some(reason) = self.undo.last_error() {
            return Err(reason.to_owned());
        }
        Ok(self.undo.revision() != revision)
    }

    fn clear_cells(&mut self, cells: &[(usize, usize)]) -> usize {
        let zeros: Vec<(usize, usize, f64)> = cells
            .iter()
            .filter(|(r, _)| *r < self.model.nodes.len())
            .map(|(r, c)| (*r, *c, 0.0))
            .collect();
        let updates = self.coord_updates(&zeros);
        let children: Vec<Box<dyn EditCommand>> = updates
            .iter()
            .map(|(row, coord)| {
                Box::new(SetNodeCoord {
                    node: NodeId(*row as u32),
                    coord: *coord,
                }) as Box<dyn EditCommand>
            })
            .collect();
        self.run_all(children, "節点座標のクリア");
        zeros.len()
    }

    fn can_append_rows(&self) -> bool {
        true
    }

    fn can_delete_rows(&self) -> bool {
        true
    }

    fn validate_row_deletion(&self, row: usize) -> Result<(), String> {
        let id = NodeId(row as u32);
        if self.model.node_in_use(id) {
            Err(
                "部材・荷重などから参照されているため削除できません（先に参照を解消してください）"
                    .to_string(),
            )
        } else {
            Ok(())
        }
    }

    fn delete_rows(&mut self, rows: &[usize]) {
        let mut sorted: Vec<usize> = rows
            .iter()
            .copied()
            .filter(|r| *r < self.model.nodes.len())
            .collect();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        sorted.dedup();
        let label = format!("節点 {} 行の削除", sorted.len());
        let children: Vec<Box<dyn EditCommand>> = sorted
            .iter()
            .map(|r| {
                Box::new(DeleteNode {
                    id: NodeId(*r as u32),
                }) as Box<dyn EditCommand>
            })
            .collect();
        self.run_all(children, &label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{parse_tsv, plan_paste, rect_to_tsv, CellRef, SelRect};

    #[test]
    fn exact_node_tsv_round_trip_and_blank_short_rows() {
        let values: [[f64; 3]; 2] = [
            [1.2345678901234567, -10.0, 3e3],
            [0.0, 1.23456789012345, 7.85e-9],
        ];
        let mut model = Model::default();
        let mut undo = UndoStack::new();
        let mut adapter = NodeGridAdapter {
            model: &mut model,
            undo: &mut undo,
            edited: false,
        };
        let source = rect_to_tsv(
            SelRect {
                r0: 0,
                r1: 1,
                c0: 0,
                c1: 2,
            },
            |r, c| values[r][c].to_string(),
        );
        let block = parse_tsv(&(source + "\r\n"));
        let plan = plan_paste(&block, CellRef { row: 0, col: 0 }, 0, 3, |r, c, t| {
            adapter.validate_cell(r, c, t)
        })
        .unwrap();
        adapter.apply_block(&plan.set, plan.extra_rows).unwrap();
        for (r, expected) in values.iter().enumerate() {
            for (c, value) in expected.iter().enumerate() {
                assert_eq!(adapter.model.nodes[r].coord[c].to_bits(), value.to_bits());
            }
        }
        let tsv = rect_to_tsv(
            SelRect {
                r0: 0,
                r1: 1,
                c0: 0,
                c1: 2,
            },
            |r, c| adapter.cell_text(r, c),
        );
        let copy = parse_tsv(&tsv);
        for (r, expected) in values.iter().enumerate() {
            for (c, value) in expected.iter().enumerate() {
                assert_eq!(
                    copy[r][c].parse::<f64>().unwrap().to_bits(),
                    value.to_bits()
                );
            }
        }
        let block = parse_tsv("\t0\r\n4\r\n");
        let plan = plan_paste(&block, CellRef { row: 0, col: 0 }, 2, 3, |r, c, t| {
            adapter.validate_cell(r, c, t)
        })
        .unwrap();
        adapter.apply_block(&plan.set, 0).unwrap();
        assert_eq!(adapter.model.nodes[0].coord, [values[0][0], 0.0, 3e3]);
        assert_eq!(
            adapter.model.nodes[1].coord,
            [4.0, values[1][1], values[1][2]]
        );
    }

    #[test]
    fn invalid_node_values_are_rejected_at_plan_and_apply_preserving_redo() {
        let mut model = Model::default();
        let mut undo = UndoStack::new();
        let mut adapter = NodeGridAdapter {
            model: &mut model,
            undo: &mut undo,
            edited: false,
        };
        adapter.apply_block(&[(0, 0, "1".into())], 1).unwrap();
        adapter.undo.undo(adapter.model);
        let original = adapter.model.clone();
        let revision = adapter.undo.revision();
        adapter.edited = false;
        for invalid in [
            "NaN", "inf", "-inf", "∞", "1e309", "1,000", "=1+2", "10mm", "abc",
        ] {
            let block = parse_tsv(&format!("2\t{invalid}"));
            let error = plan_paste(&block, CellRef { row: 0, col: 0 }, 0, 3, |r, c, t| {
                adapter.validate_cell(r, c, t)
            })
            .unwrap_err();
            assert!(error[0].contains("ブロック1行2列目"));
            assert!(error[0].contains("節点ID 0"));
            assert!(adapter
                .apply_block(&[(0, 0, "2".into()), (0, 1, invalid.into())], 1)
                .is_err());
            assert_eq!(
                rmp_serde::to_vec_named(adapter.model).unwrap(),
                rmp_serde::to_vec_named(&original).unwrap()
            );
            assert_eq!(adapter.undo.revision(), revision);
            assert!(adapter.undo.can_redo());
            assert!(!adapter.edited);
        }
    }

    #[test]
    fn fifteen_digit_scientific_tsv_fixture_meets_relative_tolerance() {
        let expected: [[f64; 3]; 2] = [[1.23456789012345, -10.0, 3000.0], [0.0, 10.0, -3000.0]];
        let block = parse_tsv(include_str!("../../tests/fixtures/tsv/excel_nodes.tsv"));
        assert_eq!(block.len(), 2);
        assert!(block.iter().all(|row| row.len() == 3));
        let mut model = Model::default();
        let mut undo = UndoStack::new();
        let mut adapter = NodeGridAdapter {
            model: &mut model,
            undo: &mut undo,
            edited: false,
        };
        let plan = plan_paste(&block, CellRef { row: 0, col: 0 }, 0, 3, |r, c, t| {
            adapter.validate_cell(r, c, t)
        })
        .unwrap();
        assert!(adapter.apply_block(&plan.set, plan.extra_rows).unwrap());
        for (node, values) in adapter.model.nodes.iter().zip(expected) {
            for (actual, value) in node.coord.iter().zip(values) {
                if value == 0.0 {
                    assert_eq!(*actual, 0.0);
                } else {
                    assert!((actual - value).abs() <= 1e-14 * value.abs());
                }
            }
        }
    }
}
