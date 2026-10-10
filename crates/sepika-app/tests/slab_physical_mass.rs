#[path = "../../sepika-job/tests/fixtures/steel_slab.rs"]
mod fixture;
use sepika_core::{ids::*, model::*};
use sepika_edit::{MaterialField, SetMaterialField, SetSlabSection, UndoStack};
use sepika_job::{prepare::prepare_model, weight_preparation::weights_are_current};

fn total(model: &Model) -> f64 {
    model
        .stories
        .iter()
        .filter_map(|s| s.dynamic_mass)
        .map(|m| m.mass_equiv_weight_n)
        .sum()
}
fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-7,
        "actual={actual}, expected={expected}"
    );
}
#[test]
fn density_thickness_reference_edit_undo_and_preparation_keep_physical_mass_current() {
    for attached in [false, true] {
        let mut model = fixture::one_cubic_metre(attached);
        let mut thicker = model.sections[2].clone();
        thicker.id = SectionId(3);
        thicker.thickness = Some(200.0);
        model.sections.push(thicker);
        let settings = Default::default();
        prepare_model(&mut model, &settings, None, true).unwrap();
        let mut undo = UndoStack::new();
        for (command, expected) in [
            (
                Box::new(SetMaterialField {
                    id: MaterialId(1),
                    field: MaterialField::Density,
                    value: Some(3.925e-9),
                }) as Box<dyn sepika_edit::EditCommand>,
                38491.10125,
            ),
            (
                Box::new(SetSlabSection {
                    id: SlabId(0),
                    section: Some(SectionId(3)),
                }),
                153964.405,
            ),
        ] {
            assert!(undo.run(&mut model, command));
            assert!(!weights_are_current(&model, settings.mass_method));
            prepare_model(&mut model, &settings, None, false).unwrap();
            close(total(&model), expected);
            undo.undo(&mut model);
            assert!(!weights_are_current(&model, settings.mass_method));
            prepare_model(&mut model, &settings, None, false).unwrap();
            close(total(&model), 76982.2025);
        }
        let manual = NodalLoad::manual(NodeId(2), [0.0, 0.0, -100.0, 0.0, 0.0, 0.0]);
        model
            .load_cases
            .iter_mut()
            .find(|c| c.name == DL_CASE_NAME)
            .unwrap()
            .nodal
            .push(manual.clone());
        prepare_model(&mut model, &settings, None, false).unwrap();
        close(total(&model), 77082.2025);
        assert!(undo.run(
            &mut model,
            Box::new(SetSlabSection {
                id: SlabId(0),
                section: None
            })
        ));
        assert!(prepare_model(&mut model, &settings, None, false).is_err());
        assert!(!weights_are_current(&model, settings.mass_method));
        assert!(model
            .load_cases
            .iter()
            .find(|c| c.name == DL_CASE_NAME)
            .unwrap()
            .nodal
            .contains(&manual));
        undo.undo(&mut model);
        prepare_model(&mut model, &settings, None, false).unwrap();
        close(total(&model), 77082.2025);
    }
}
