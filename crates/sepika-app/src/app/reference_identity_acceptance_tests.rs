use super::*;
use sepika_core::ids::{WallPlateAssignmentRegionId, WallPlateId};
use sepika_core::model::*;

fn path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "sepika678-acceptance-{label}-{}.ovika",
        std::process::id()
    ))
}

fn bytes(value: &impl serde::Serialize) -> Vec<u8> {
    rmp_serde::to_vec_named(value).unwrap()
}

#[derive(Debug, PartialEq)]
struct RetainedState {
    model: Vec<u8>,
    results: Vec<u8>,
    preparation: Vec<u8>,
    generation: Vec<u8>,
    revision: u64,
    undo: Option<String>,
    redo: Option<String>,
    selection: String,
    project_path: Option<std::path::PathBuf>,
    staleness: String,
}

fn retained(app: &App) -> RetainedState {
    RetainedState {
        model: bytes(&app.core.model),
        results: bytes(&app.core.scoped.results),
        preparation: bytes(&app.core.scoped.preparation),
        generation: app.result_input(&result_validity::ResultInputKey::Modal),
        revision: app.core.scoped.undo.revision(),
        undo: app.core.scoped.undo.undo_label().map(str::to_owned),
        redo: app.core.scoped.undo.redo_label().map(str::to_owned),
        selection: format!("{:?}", app.ui.scoped.selection),
        project_path: app.core.scoped.project_path.clone(),
        staleness: format!("{:?}", app.core.scoped.staleness),
    }
}

fn existing_app() -> App {
    let mut app = App::default();
    let mut model = crate::sample::portal_frame();
    model.panel_zone = PanelZoneMode::None;
    assert!(app.load_model(model));
    app.core.analysis_cfg.threads = 1;
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.results.is_some(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(app.core.scoped.preparation.is_some());
    for x_mm in [-100., -200.] {
        app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(sepika_edit::SetNodeCoord {
                node: NodeId(0),
                coord: [x_mm, 0., 0.],
            }),
        );
    }
    app.core.scoped.undo.undo(&mut app.core.model);
    assert!(app.core.scoped.undo.can_undo() && app.core.scoped.undo.can_redo());
    app.core.scoped.staleness.mark_edited();
    app.core.scoped.project_path = Some(path("current"));
    app.ui
        .scoped
        .selection
        .select_nodes(vec![NodeId(0)], Some(NodeId(0)));
    app
}

#[test]
fn reference_identity_rejected_load_and_correct_hash_open_preserve_app_state_and_history() {
    let mut app = existing_app();
    let before = retained(&app);
    let mut invalid = crate::sample::portal_frame();
    invalid.elements[0].section = Some(SectionId(678));
    let file = path("invalid-open");
    super::secondary_cft_tests::write_invalid_container(&file, &invalid);
    let original_file = std::fs::read(&file).unwrap();
    for open_file in [false, true] {
        if open_file {
            app.open_project_from(file.clone());
        } else {
            assert!(!app.load_model(invalid.clone()));
        }
        let reason = app.core.scoped.last_error.as_ref().unwrap();
        assert!(reason.contains("Elem 0 -> Section 678"), "{reason}");
        assert_eq!(retained(&app), before);
        assert_eq!(std::fs::read(&file).unwrap(), original_file);
    }
    app.core.scoped.undo.undo(&mut app.core.model);
    assert_eq!(app.core.model.nodes[0].coord[0], 0.);
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_eq!(bytes(&app.core.model), before.model);
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_eq!(app.core.model.nodes[0].coord[0], -200.);
    std::fs::remove_file(file).unwrap();
}

fn pushover(qu: f64) -> sepika_solver::nonlinear::pushover::PushoverResult {
    use sepika_solver::nonlinear::pushover::*;
    PushoverResult {
        wall_capacities: vec![],
        rc_wall_shear_method: Default::default(),
        steps: vec![],
        wall_history: None,
        wall_run: None,
        confirmed_history: None,
        ds_evaluation: None,
        capacity_evaluation: None,
        capacity_curve: vec![],
        hinges: vec![],
        shear_yields: vec![],
        mechanism: MechanismType::Partial,
        qu,
        member_response: vec![],
        control: Default::default(),
        member_history: vec![],
        fiber_states: vec![],
        termination: Default::default(),
    }
}

#[test]
fn reference_identity_save_rejection_precedes_result_sync_and_preserves_existing_file() {
    let mut app = existing_app();
    let file = path("existing-save");
    sepika_io::ovika::save_ovika(&file, &Model::default(), Default::default()).unwrap();
    let original_file = std::fs::read(&file).unwrap();
    app.core.model.elements[0].section = Some(SectionId(678));
    let bundle = app.core.scoped.results.as_mut().unwrap();
    bundle.pushover = Some(pushover(11.));
    bundle.pushover_x = Some(pushover(22.));
    bundle.pushover_y = Some(pushover(33.));
    app.core.scoped.pushover_view_dir = SeismicDir::Y;
    let before = retained(&app);
    app.save_project_to_opts(file.clone(), Some(false));
    let reason = app.core.scoped.last_error.as_ref().unwrap();
    assert!(reason.contains("Elem 0 -> Section 678"), "{reason}");
    assert_eq!(retained(&app), before);
    assert_eq!(std::fs::read(&file).unwrap(), original_file);
    assert!(sepika_io::ovika::load_ovika(&file)
        .unwrap()
        .model
        .elements
        .is_empty());
    app.sync_pushover_for_save();
    assert_eq!(
        app.core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .pushover
            .as_ref()
            .unwrap()
            .qu,
        33.
    );
    assert_ne!(bytes(&app.core.scoped.results), before.results);
    std::fs::remove_file(file).unwrap();
}

#[test]
fn reference_identity_incomplete_models_save_and_load_without_gui_completion_requirements() {
    for no_section in [false, true] {
        let mut model = crate::sample::portal_frame();
        if no_section {
            for element in &mut model.elements {
                element.section = None;
            }
        } else {
            for section in &mut model.sections {
                section.frame_use = None;
            }
            assert!(model.validate().is_err());
        }
        model.validate_reference_ids().unwrap();
        let mut app = App::default();
        assert!(app.load_model(model));
        let before = bytes(&app.core.model);
        let file = path(if no_section { "none" } else { "frame-use-none" });
        app.save_project_to(file.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        assert_eq!(bytes(&app.core.model), before);
        assert_eq!(
            bytes(&sepika_io::ovika::load_ovika(&file).unwrap().model),
            before
        );
        assert!(!app.core.scoped.staleness.unsaved_changes);
        if !no_section {
            let mut reopened = App::default();
            reopened.open_project_from(file.clone());
            assert!(reopened.core.scoped.last_error.is_some());
            assert!(reopened.core.model.elements.is_empty());
        }
        std::fs::remove_file(file).unwrap();
    }
}

#[test]
fn reference_identity_wall_plate_without_parent_wall_region_keeps_stable_assignment_id() {
    let mut model = Model::default();
    for (index, coord) in [
        [0., 0., 0.],
        [1000., 0., 0.],
        [1000., 0., 1000.],
        [0., 0., 1000.],
    ]
    .into_iter()
    .enumerate()
    {
        model.nodes.push(Node {
            id: NodeId(index as u32),
            coord,
            restraint: Default::default(),
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    model.add_enclosed_wall_plate_from_nodes(
        &[NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
        WallPlate {
            id: WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: None,
            self_weight_shares: vec![],
            opening_area: 0.,
            opening_weight: 0.,
            openings: vec![],
            loads: vec![],
            slit: Default::default(),
            dl_support: None,
        },
    );
    model.wall_assignment_regions.regions[0].id = WallPlateAssignmentRegionId(632);
    assert!(model.wall_regions.is_empty());
    model.validate_reference_ids().unwrap();
    assert!(model.wall_plates[0].has_quad_boundary(&model));
    let file = path("parentless-wall");
    sepika_io::ovika::save_ovika(&file, &model, Default::default()).unwrap();
    let loaded = sepika_io::ovika::load_ovika(&file).unwrap().model;
    assert_eq!(bytes(&loaded), bytes(&model));
    std::fs::remove_file(file).unwrap();
}
