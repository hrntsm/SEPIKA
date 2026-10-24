use super::node_grid::NodeGridAdapter;
use crate::grid::{parse_tsv, plan_paste, rect_to_tsv, CellRef, GridAdapter, SelRect};
use sepika_core::model::Model;
use sepika_edit::UndoStack;

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
    assert!(adapter.apply_block(&plan.set, plan.extra_rows).unwrap());
    for (r, expected) in values.iter().enumerate() {
        for (c, value) in expected.iter().enumerate() {
            assert_eq!(adapter.model.nodes[r].coord[c].to_bits(), value.to_bits());
            assert_eq!(
                adapter.cell_text(r, c).parse::<f64>().unwrap().to_bits(),
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
fn invalid_node_values_are_rejected_at_apply_preserving_history() {
    let mut model = Model::default();
    let mut undo = UndoStack::new();
    let mut adapter = NodeGridAdapter {
        model: &mut model,
        undo: &mut undo,
        edited: false,
    };
    adapter.apply_block(&[(0, 0, "1".into())], 1).unwrap();
    adapter.apply_block(&[(1, 0, "2".into())], 1).unwrap();
    adapter.undo.undo(adapter.model);
    adapter.edited = false;
    let original = rmp_serde::to_vec_named(adapter.model).unwrap();
    let revision = adapter.undo.revision();
    for invalid in [
        "NaN", "Infinity", "inf", "-inf", "∞", "1e309", "1,000", "=1+2", "10mm", "abc",
    ] {
        assert!(adapter
            .apply_block(&[(0, 0, "2".into()), (1, 1, invalid.into())], 1)
            .is_err());
        assert_eq!(rmp_serde::to_vec_named(adapter.model).unwrap(), original);
        assert_eq!(adapter.undo.revision(), revision);
        assert!(adapter.undo.can_undo());
        assert!(adapter.undo.can_redo());
        assert!(!adapter.edited);
    }
    for (cells, append) in [
        (vec![(0, 3, "1".into())], 0),
        (vec![(1, 0, "1".into())], 0),
        (vec![(0, 0, "1".into())], 1),
        (vec![], 1),
        (vec![(usize::MAX, 0, "1".into())], 1),
        (
            vec![(0, 0, "1".into()); crate::grid::MAX_PASTE_CELLS + 1],
            0,
        ),
    ] {
        assert!(adapter.apply_block(&cells, append).is_err());
        assert_eq!(rmp_serde::to_vec_named(adapter.model).unwrap(), original);
        assert_eq!(adapter.undo.revision(), revision);
        assert!(adapter.undo.can_redo());
        assert!(!adapter.edited);
    }
}

#[cfg(feature = "gui")]
#[test]
fn paste_events_through_nodes_table_preserve_current_results_and_history_on_rejection() {
    use super::{result_validity::ResultInputKey, App, StaticCaseKey};
    use crate::grid::{GridState, MAX_PASTE_CELLS};
    use sepika_core::{dof::Dof6Mask, ids::LoadCaseId};
    let mut app = App::default();
    app.load_model(crate::sample::portal_frame());
    app.run_linear_static(LoadCaseId(0));
    assert!(app.core.scoped.undo.run(
        &mut app.core.model,
        Box::new(sepika_edit::AddNode {
            coord: [9000.0, 0.0, 0.0],
            restraint: Dof6Mask::FREE,
        })
    ));
    app.core.scoped.undo.undo(&mut app.core.model);
    let key = ResultInputKey::Static(StaticCaseKey::User(LoadCaseId(0)));
    assert!(
        app.require_result_input(key.clone()).is_ok(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(app.core.scoped.undo.can_undo());
    assert!(app.core.scoped.undo.can_redo());
    app.core.scoped.staleness.unsaved_changes = false;
    let original = rmp_serde::to_vec_named(&app.core.model).unwrap();
    let revision = app.core.scoped.undo.revision();
    let undo_label = app.core.scoped.undo.undo_label().map(str::to_owned);
    let redo_label = app.core.scoped.undo.redo_label().map(str::to_owned);
    let results = format!("{:?}", app.core.scoped.results);
    let staleness = format!("{:?}", app.core.scoped.staleness);
    let ctx = egui::Context::default();
    let mut invalid = [
        "NaN",
        "Infinity",
        "-Infinity",
        "1e309",
        "abc",
        "1,000",
        "=1+2",
        "10mm",
    ]
    .map(|s| (format!("25\t{s}"), 0))
    .to_vec();
    invalid.extend([
        ("25\t\t\t".into(), 0),
        ("25\t ".into(), 2),
        ("\t\t\t".into(), 0),
        ("1\n".repeat(MAX_PASTE_CELLS + 1), 0),
    ]);
    for (text, col) in invalid {
        app.ui.scoped.node_grid.grid = GridState::new(app.core.model.nodes.len() + 1, 3);
        app.ui
            .scoped
            .node_grid
            .grid
            .click(CellRef { row: 0, col }, false);
        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Paste(text)],
                ..Default::default()
            },
            |ui| {
                crate::tables::nodes::nodes_table(ui, &mut app);
            },
        );
        assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), original);
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert_eq!(app.core.scoped.undo.undo_label(), undo_label.as_deref());
        assert_eq!(app.core.scoped.undo.redo_label(), redo_label.as_deref());
        assert!(app.core.scoped.undo.can_undo());
        assert!(app.core.scoped.undo.can_redo());
        assert_eq!(format!("{:?}", app.core.scoped.results), results);
        assert_eq!(format!("{:?}", app.core.scoped.staleness), staleness);
        assert!(app.require_result_input(key.clone()).is_ok());
    }
    let end = app.core.model.nodes.len();
    app.ui.scoped.node_grid.grid = GridState::new(end + 1, 3);
    app.ui
        .scoped
        .node_grid
        .grid
        .click(CellRef { row: end, col: 0 }, false);
    let _ = ctx.run_ui(
        egui::RawInput {
            events: vec![egui::Event::Paste("+1.5\t-2\t3e3\r\n".into())],
            ..Default::default()
        },
        |ui| {
            crate::tables::nodes::nodes_table(ui, &mut app);
        },
    );
    assert_eq!(app.core.model.nodes[end].coord, [1.5, -2.0, 3000.0]);
    assert_eq!(app.core.scoped.undo.revision(), revision + 1);
    assert!(!app.core.scoped.undo.can_redo());
    assert!(app.core.scoped.staleness.unsaved_changes);
    assert!(app.require_result_input(key).is_err());
    app.core.scoped.undo.undo(&mut app.core.model);
    assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), original);
}

#[test]
fn fifteen_digit_scientific_tsv_fixture_meets_relative_tolerance() {
    let expected: [[f64; 3]; 2] = [[1.23456789012345, -10.0, 3000.0], [0.0, 10.0, -3000.0]];
    let block = parse_tsv(include_str!("../../tests/fixtures/tsv/excel_nodes.tsv"));
    assert_eq!(block.len(), 2);
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
