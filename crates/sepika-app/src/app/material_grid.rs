//! 材料定数の数値グリッド。行追加とFc/FyのTSV編集には対応しない。

use crate::grid::GridAdapter;
use sepika_core::model::Model;
use sepika_edit::{CompositeCommand, MaterialField, SetMaterialField, UndoStack};

pub struct MaterialGridAdapter<'a> {
    pub model: &'a mut Model,
    pub undo: &'a mut UndoStack,
    pub edited: bool,
}

fn field(col: usize) -> Result<MaterialField, String> {
    match col {
        0 => Ok(MaterialField::Young),
        1 => Ok(MaterialField::Poisson),
        2 => Ok(MaterialField::Density),
        3 => Ok(MaterialField::StrengthFactor),
        _ => Err("TSV対象外の列です（Fc/Fyを含む）".into()),
    }
}

impl GridAdapter for MaterialGridAdapter<'_> {
    fn rows(&self) -> usize {
        self.model.materials.len()
    }
    fn cols(&self) -> usize {
        4
    }
    fn row_label(&self, row: usize) -> String {
        self.model
            .materials
            .get(row)
            .map(|m| m.id.0.to_string())
            .unwrap_or_default()
    }
    fn cell_text(&self, row: usize, col: usize) -> String {
        let Some(mat) = self.model.materials.get(row) else {
            return String::new();
        };
        let value = match field(col) {
            Ok(MaterialField::Young) => Some(mat.young),
            Ok(MaterialField::Poisson) => Some(mat.poisson),
            Ok(MaterialField::Density) => Some(mat.density),
            Ok(MaterialField::StrengthFactor) => mat.strength_factor,
            _ => None,
        };
        value.map(|v| v.to_string()).unwrap_or_default()
    }
    fn validate_cell(&self, row: usize, col: usize, text: &str) -> Result<(), String> {
        let reason = || -> Result<(), String> {
            let mat = self
                .model
                .materials
                .get(row)
                .filter(|m| m.id.index() == row)
                .ok_or_else(|| "材料が存在しません".to_string())?;
            let field = field(col)?;
            if field == MaterialField::Young && mat.is_standard_concrete() {
                return Err("標準普通コンクリートのEは編集できません".into());
            }
            match text.trim().parse::<f64>() {
                Ok(value) if value.is_finite() => Ok(()),
                _ => Err("有限の数値を入力してください".into()),
            }
        };
        reason().map_err(|reason| format!("材料ID {row}: {reason}"))
    }
    fn apply_block(
        &mut self,
        cells: &[(usize, usize, String)],
        append_rows: usize,
    ) -> Result<bool, String> {
        if append_rows != 0 {
            return Err("材料の行追加には対応していません".into());
        }
        for (r, c, text) in cells {
            self.validate_cell(*r, *c, text)?;
        }
        if cells.is_empty() {
            return Ok(false);
        }
        let children = cells
            .iter()
            .map(|(r, c, text)| {
                Box::new(SetMaterialField {
                    id: self.model.materials[*r].id,
                    field: field(*c).expect("検証済みの対象列"),
                    value: Some(text.trim().parse().expect("検証済みの数値")),
                }) as Box<dyn sepika_edit::EditCommand>
            })
            .collect();
        let changed = self.undo.run(
            self.model,
            Box::new(CompositeCommand {
                label: "材料定数の貼り付け".into(),
                children,
            }),
        );
        self.edited |= changed;
        if let Some(reason) = self.undo.last_error() {
            return Err(reason.to_owned());
        }
        Ok(changed)
    }
    fn clear_cells(&mut self, cells: &[(usize, usize)]) -> usize {
        let values = cells
            .iter()
            .map(|(r, c)| (*r, *c, "0".into()))
            .collect::<Vec<_>>();
        if self.apply_block(&values, 0) == Ok(true) {
            cells.len()
        } else {
            0
        }
    }
    fn can_append_rows(&self) -> bool {
        false
    }
    fn can_delete_rows(&self) -> bool {
        false
    }
    fn validate_row_deletion(&self, _row: usize) -> Result<(), String> {
        Err("材料の削除は一覧の削除ボタンを使ってください".into())
    }
    fn delete_rows(&mut self, _rows: &[usize]) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{parse_tsv, plan_paste, rect_to_tsv, CellRef, SelRect};

    fn model() -> Model {
        crate::sample::portal_frame()
    }

    #[test]
    fn canonical_material_tsv_and_atomic_undo_redo() {
        let mut model = model();
        let original = model.clone();
        let mut undo = UndoStack::new();
        let mut adapter = MaterialGridAdapter {
            model: &mut model,
            undo: &mut undo,
            edited: false,
        };
        let tsv = rect_to_tsv(
            SelRect {
                r0: 0,
                r1: 0,
                c0: 0,
                c1: 3,
            },
            |r, c| adapter.cell_text(r, c),
        );
        let block = parse_tsv(&tsv);
        assert_eq!(
            block[0][2].parse::<f64>().unwrap().to_bits(),
            original.materials[0].density.to_bits()
        );
        let block = parse_tsv("200123.45678901234\t0.29\t7.85e-9\t1.05\r\n");
        let plan = plan_paste(
            &block,
            CellRef { row: 0, col: 0 },
            adapter.rows(),
            adapter.cols(),
            |r, c, t| adapter.validate_cell(r, c, t),
        )
        .unwrap();
        assert!(adapter.apply_block(&plan.set, 0).unwrap());
        assert!(adapter.edited);
        let changed = adapter.model.clone();
        assert_eq!(
            changed.materials[0].young.to_bits(),
            200123.45678901234_f64.to_bits()
        );
        assert_eq!(changed.materials[0].density, 7.85e-9);
        assert_eq!(changed.materials[0].strength_factor, Some(1.05));
        assert_eq!(adapter.undo.revision(), 1);
        adapter.undo.undo(adapter.model);
        assert_eq!(
            rmp_serde::to_vec_named(adapter.model).unwrap(),
            rmp_serde::to_vec_named(&original).unwrap()
        );
        assert!(!adapter.undo.can_undo());
        adapter.undo.redo(adapter.model);
        assert_eq!(
            rmp_serde::to_vec_named(adapter.model).unwrap(),
            rmp_serde::to_vec_named(&changed).unwrap()
        );
    }

    #[test]
    fn invalid_material_blocks_preserve_model_and_redo() {
        let mut model = model();
        let mut undo = UndoStack::new();
        let mut adapter = MaterialGridAdapter {
            model: &mut model,
            undo: &mut undo,
            edited: false,
        };
        adapter.apply_block(&[(0, 1, "0.2".into())], 0).unwrap();
        adapter.undo.undo(adapter.model);
        let original = adapter.model.clone();
        let revision = adapter.undo.revision();
        adapter.edited = false;
        for invalid in [
            "NaN", "inf", "-inf", "∞", "1e309", "1,234", "=1+2", "10 mm", "abc",
        ] {
            let cells = [(0, 0, "210000".into()), (0, 2, invalid.into())];
            let error = adapter.apply_block(&cells, 0).unwrap_err();
            assert!(error.contains("材料ID 0"));
            assert_eq!(
                rmp_serde::to_vec_named(adapter.model).unwrap(),
                rmp_serde::to_vec_named(&original).unwrap()
            );
            assert_eq!(adapter.undo.revision(), revision);
            assert!(adapter.undo.can_redo());
            assert!(!adapter.edited);
        }
        for cells in [vec![(0, 4, "24".into())], vec![(999, 0, "1".into())]] {
            assert!(adapter.apply_block(&cells, 0).is_err());
            assert_eq!(
                rmp_serde::to_vec_named(adapter.model).unwrap(),
                rmp_serde::to_vec_named(&original).unwrap()
            );
        }
        assert!(adapter.apply_block(&[(0, 0, "1".into())], 1).is_err());
        assert_eq!(
            rmp_serde::to_vec_named(adapter.model).unwrap(),
            rmp_serde::to_vec_named(&original).unwrap()
        );
    }

    #[test]
    fn apply_rechecks_material_identity_and_young_editability() {
        let mut model = model();
        let mut other = model.materials[0].clone();
        other.id = sepika_core::ids::MaterialId(1);
        model.materials.push(other);
        let mut undo = UndoStack::new();
        let mut adapter = MaterialGridAdapter {
            model: &mut model,
            undo: &mut undo,
            edited: false,
        };
        let cells: [(usize, usize, String); 2] = [(0, 0, "210000".into()), (1, 0, "220000".into())];
        for (r, c, t) in &cells {
            adapter.validate_cell(*r, *c, t).unwrap();
        }
        adapter.model.materials[1].category = sepika_core::model::MaterialCategory::Concrete;
        adapter.model.materials[1].concrete_class = sepika_core::units::ConcreteClass::Normal;
        let original = adapter.model.clone();
        assert!(adapter
            .apply_block(&cells, 0)
            .unwrap_err()
            .contains("材料ID 1"));
        assert_eq!(
            rmp_serde::to_vec_named(adapter.model).unwrap(),
            rmp_serde::to_vec_named(&original).unwrap()
        );
        assert!(!adapter.undo.can_undo());
        adapter.model.materials[1].concrete_class = sepika_core::units::ConcreteClass::UserDefined;
        assert!(adapter.apply_block(&cells, 0).unwrap());
        adapter.model.materials[1].id = sepika_core::ids::MaterialId(50);
        let original = adapter.model.clone();
        assert!(adapter.apply_block(&cells, 0).is_err());
        assert_eq!(
            rmp_serde::to_vec_named(adapter.model).unwrap(),
            rmp_serde::to_vec_named(&original).unwrap()
        );
    }

    #[test]
    fn blank_cells_short_rows_and_explicit_zero_are_distinct() {
        let mut model = model();
        let original = model.clone();
        let mut undo = UndoStack::new();
        let mut adapter = MaterialGridAdapter {
            model: &mut model,
            undo: &mut undo,
            edited: false,
        };
        let block = parse_tsv("\t0\t\t\r\n");
        let plan = plan_paste(
            &block,
            CellRef { row: 0, col: 0 },
            adapter.rows(),
            adapter.cols(),
            |r, c, t| adapter.validate_cell(r, c, t),
        )
        .unwrap();
        assert_eq!(plan.skipped_empty, 3);
        adapter.apply_block(&plan.set, 0).unwrap();
        assert_eq!(
            adapter.model.materials[0].young,
            original.materials[0].young
        );
        assert_eq!(adapter.model.materials[0].poisson, 0.0);
        assert_eq!(
            adapter.model.materials[0].density,
            original.materials[0].density
        );
        let revision = adapter.undo.revision();
        assert!(!adapter.apply_block(&[], 0).unwrap());
        assert_eq!(adapter.undo.revision(), revision);
    }
}
