use sepika_app::app::App;
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::StoryId;
use sepika_core::model::{Constraint, MassMethod, Model};
use sepika_edit::{ApplyPreparedModel, DeleteStory, UndoStack};
use sepika_job::weight_preparation::{apply_generated_weights, weights_are_current};

fn assert_ovika_roundtrip(model: &Model, label: &str) {
    model.validate_reference_ids().unwrap();
    let bytes = rmp_serde::to_vec_named(model).unwrap();
    let path = std::env::temp_dir().join(format!(
        "sepika-story-lifecycle-{}-{label}.ovika",
        std::process::id()
    ));
    sepika_io::ovika::save_ovika(&path, model, Default::default()).unwrap();
    let restored = sepika_io::ovika::load_ovika(&path).unwrap().model;
    assert_eq!(rmp_serde::to_vec_named(&restored).unwrap(), bytes);
    assert_eq!(rmp_serde::to_vec_named(model).unwrap(), bytes);
    std::fs::remove_file(path).unwrap();
}

fn assert_restored(model: &Model, expected: &Model, label: &str) {
    assert!(model.eq_ignoring_dofmap(expected), "{label}");
    assert_eq!(
        model.seismic_weight_generation,
        expected.seismic_weight_generation
    );
    assert_ovika_roundtrip(model, label);
}

#[test]
fn app_generation_story_reduction_and_ordered_undo_redo_restore_complete_models() {
    let mut app = App::default();
    app.load_model(sepika_app::sample::portal_frame());
    let initial = app.core.model.clone();
    app.generate_stories_action();
    assert!(app.core.scoped.last_error.is_none());
    assert_eq!(app.core.scoped.undo.undo_label(), Some("準備計算の採用"));
    assert_eq!(app.core.model.stories.len(), 2);
    let record = app.core.model.seismic_weight_generation.as_ref().unwrap();
    assert_eq!(record.calculated_weights.len(), 2);
    assert_eq!(record.automatic_diaphragms.len(), 2);
    assert_eq!(record.automatic_master_restraints.len(), 2);
    assert!(weights_are_current(
        &app.core.model,
        app.core.model.mass_method
    ));
    assert_ovika_roundtrip(&app.core.model, "app-generated");
    let generated = app.core.model.clone();

    app.core.scoped.undo.undo(&mut app.core.model);
    assert_restored(&app.core.model, &initial, "app-undo-generation");
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_restored(&app.core.model, &generated, "app-redo-generation");

    assert!(app.core.scoped.undo.run(
        &mut app.core.model,
        Box::new(DeleteStory { story: StoryId(1) })
    ));
    let deleted = app.core.model.clone();
    assert_eq!(deleted.stories.len(), 1);
    assert!(!weights_are_current(&deleted, deleted.mass_method));
    assert_ovika_roundtrip(&deleted, "app-deleted");
    app.generate_stories_action();
    assert!(app.core.scoped.last_error.is_none());
    assert_eq!(app.core.model.stories.len(), 1);
    let record = app.core.model.seismic_weight_generation.as_ref().unwrap();
    assert_eq!(record.calculated_weights.len(), 1);
    assert_eq!(record.calculated_weights[0].0, StoryId(0));
    assert!(record
        .automatic_diaphragms
        .iter()
        .all(|constraint| matches!(
            constraint,
            Constraint::RigidDiaphragm {
                story: StoryId(0),
                ..
            }
        )));
    assert!(weights_are_current(
        &app.core.model,
        app.core.model.mass_method
    ));
    assert_ovika_roundtrip(&app.core.model, "app-reduced");
    let reduced = app.core.model.clone();

    app.core.scoped.undo.undo(&mut app.core.model);
    assert_restored(&app.core.model, &deleted, "app-undo-regeneration");
    app.core.scoped.undo.undo(&mut app.core.model);
    assert_restored(&app.core.model, &generated, "app-undo-deletion");
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_restored(&app.core.model, &deleted, "app-redo-deletion");
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_restored(&app.core.model, &reduced, "app-redo-regeneration");
}

#[test]
fn stale_records_with_existing_ids_and_manual_settings_survive_adoption_and_regeneration() {
    let mut model = sepika_app::sample::portal_frame();
    let method = MassMethod::CorrectedLumped;
    let generated =
        sepika_load::story_gen::generate_stories_with_opts(&model, &[], true, method).unwrap();
    apply_generated_weights(&mut model, generated, method);
    model.assign_stb_node_ids().unwrap();
    let automatic = model.clone();
    let mut manual = model.clone();
    manual.stories[1].weight_override = Some(123456.0);
    manual.stories[1].seismic_weight = Some(123456.0);
    let master = match &mut manual.constraints[1] {
        Constraint::RigidDiaphragm {
            master,
            ci_override,
            ..
        } => {
            *ci_override = Some(0.42);
            *master
        }
        _ => panic!("剛床拘束が必要"),
    };
    manual.nodes[master.index()].restraint = Dof6Mask::FIXED;
    let record = manual.seismic_weight_generation.as_mut().unwrap();
    record.input_key = vec![255];
    record.output_key = vec![254];
    let manual_constraint = manual.constraints[1].clone();
    assert!(!weights_are_current(&manual, method));
    assert!(!manual.is_automatic_seismic_diaphragm(&manual_constraint));
    assert!(!manual.is_automatic_seismic_master_restraint(master));
    assert_ovika_roundtrip(&manual, "manual-stale");

    let mut undo = UndoStack::new();
    assert!(undo.run(
        &mut model,
        Box::new(ApplyPreparedModel {
            prepared: manual.clone()
        })
    ));
    assert_restored(&model, &manual, "manual-adopted");
    let mut prepared = model.clone();
    let generated =
        sepika_load::story_gen::generate_stories_with_opts(&prepared, &[], true, method).unwrap();
    apply_generated_weights(&mut prepared, generated, method);
    assert_eq!(prepared.stories[1].weight_override, Some(123456.0));
    assert_eq!(prepared.stories[1].seismic_weight, Some(123456.0));
    assert!(prepared.constraints.contains(&manual_constraint));
    assert_eq!(prepared.nodes[master.index()].restraint, Dof6Mask::FIXED);
    assert!(weights_are_current(&prepared, method));
    let regenerated = prepared.clone();
    assert!(undo.run(&mut model, Box::new(ApplyPreparedModel { prepared })));
    assert_restored(&model, &regenerated, "manual-regenerated");

    undo.undo(&mut model);
    assert_restored(&model, &manual, "manual-undo-regeneration");
    undo.undo(&mut model);
    assert_restored(&model, &automatic, "manual-undo-adoption");
    undo.redo(&mut model);
    assert_restored(&model, &manual, "manual-redo-adoption");
    undo.redo(&mut model);
    assert_restored(&model, &regenerated, "manual-redo-regeneration");
}
