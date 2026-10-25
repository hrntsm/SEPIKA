use super::*;
use sepika_core::model::*;
use sepika_edit::{AddMemberLoad, AddNodalLoad, SetElementSection, SetNodeCoord};

fn app(prepared: bool) -> App {
    let mut model = crate::sample::portal_frame();
    model.load_cases.clear();
    model.panel_zone = PanelZoneMode::None;
    let mut app = App::default();
    assert!(app.load_model(model));
    if prepared {
        app.run_preparation();
        assert!(app.core.scoped.last_error.is_none());
        assert!(app.core.scoped.preparation.is_some());
        let case = app
            .core
            .model
            .load_cases
            .iter()
            .find(|c| c.name == "DL")
            .unwrap();
        assert!(case.member.iter().any(|load| {
            load.elem == ElemId(2)
                && load.source == LoadSource::Auto
                && matches!(
                    load.kind,
                    MemberLoadKind::Distributed {
                        a: 0.,
                        b: 6000.,
                        ..
                    }
                )
        }));
    }
    app
}

fn shorten(app: &mut App) {
    assert!(
        app.apply_model_edit(Box::new(SetNodeCoord {
            node: NodeId(3),
            coord: [3000., 0., 3500.],
        })),
        "{:?}",
        app.core.scoped.last_error
    );
}

fn path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("sepika-816-{name}-{}.ovika", std::process::id()))
}

fn save_and_open(app: &mut App, path: &std::path::Path) -> App {
    app.save_project_to(path.to_path_buf());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut opened = App::default();
    opened.open_project_from(path.to_path_buf());
    assert!(
        opened.core.scoped.last_error.is_none(),
        "{:?}",
        opened.core.scoped.last_error
    );
    opened
}

#[test]
fn shortened_beam_normal_save_preserves_inputs_without_repreparation_and_supports_undo_redo() {
    let mut app = app(true);
    app.run_linear_static(LoadCaseId(0));
    assert!(app.core.scoped.last_error.is_none());
    assert!(app.core.scoped.results.is_some());
    assert!(app.apply_model_edit(Box::new(AddMemberLoad {
        lc: LoadCaseId(0),
        load: MemberLoad::manual(
            ElemId(2),
            [0., 0., -1.],
            MemberLoadKind::Point { a: 1000., p: 123. }
        ),
    })));
    assert!(app.apply_model_edit(Box::new(AddNodalLoad {
        lc: LoadCaseId(0),
        load: NodalLoad::manual(NodeId(2), [0., 0., -456., 0., 0., 0.]),
    })));
    shorten(&mut app);
    let model_before = format!("{:?}", app.core.model);
    let revision_before = app.core.scoped.undo.revision();
    let path = path("immediate");
    assert!(sepika_io::ovika::save_ovika(&path, &app.core.model, Default::default()).is_err());
    let mut opened = save_and_open(&mut app, &path);
    assert_eq!(
        opened.core.model.node(NodeId(3)).unwrap().coord,
        [3000., 0., 3500.]
    );
    let dl = &opened.core.model.load_cases[0];
    assert_eq!(dl.name, "DL");
    assert_eq!(dl.member.len(), 1);
    assert_eq!(dl.member[0].source, LoadSource::Manual);
    assert!(matches!(
        dl.member[0].kind,
        MemberLoadKind::Point { a: 1000., p: 123. }
    ));
    assert_eq!(
        dl.nodal,
        vec![NodalLoad::manual(NodeId(2), [0., 0., -456., 0., 0., 0.])]
    );
    assert!(opened.core.model.load_cases.iter().all(|case| {
        case.nodal.iter().all(|l| !l.source.is_auto())
            && case.member.iter().all(|l| !l.source.is_auto())
    }));
    assert!(opened.core.scoped.preparation.is_none());
    assert!(opened.core.scoped.results.is_none());
    assert!(opened.core.scoped.staleness.preparation_stale);
    assert_eq!(format!("{:?}", app.core.model), model_before);
    assert_eq!(app.core.scoped.undo.revision(), revision_before);
    app.undo_action();
    assert_eq!(app.core.model.node(NodeId(3)).unwrap().coord[0], 6000.);
    app.redo_action();
    assert_eq!(app.core.model.node(NodeId(3)).unwrap().coord[0], 3000.);
    opened.run_preparation();
    assert!(
        opened.core.scoped.last_error.is_none(),
        "{:?}",
        opened.core.scoped.last_error
    );
    assert!(opened.core.model.load_cases[0].member.iter().any(|load| {
        load.source.is_auto()
            && matches!(
                load.kind,
                MemberLoadKind::Distributed {
                    a: 0.,
                    b: 3000.,
                    ..
                }
            )
    }));
    let regenerated = save_and_open(&mut opened, &path);
    assert!(regenerated.core.scoped.preparation.is_some());
    assert_eq!(
        regenerated.core.model.load_cases,
        opened.core.model.load_cases
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn candidate_write_failure_keeps_existing_file_and_in_memory_derived_data() {
    let mut app = app(true);
    app.run_linear_static(LoadCaseId(0));
    let path = path("write-failure");
    save_and_open(&mut app, &path);
    let bytes = std::fs::read(&path).unwrap();
    shorten(&mut app);
    let model_before = format!("{:?}", app.core.model);
    let prep_before = rmp_serde::to_vec_named(&app.core.scoped.preparation).unwrap();
    let results_before = rmp_serde::to_vec_named(&app.core.scoped.results).unwrap();
    let revision = app.core.scoped.undo.revision();
    let previous_path = app.core.scoped.project_path.clone();
    let tmp_path = path.with_extension("ovika.tmp");
    std::fs::create_dir(&tmp_path).unwrap();
    app.save_project_to(path.clone());
    assert!(app
        .core
        .scoped
        .last_error
        .as_ref()
        .unwrap()
        .contains("保存エラー"));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(format!("{:?}", app.core.model), model_before);
    assert_eq!(
        rmp_serde::to_vec_named(&app.core.scoped.preparation).unwrap(),
        prep_before
    );
    assert_eq!(
        rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
        results_before
    );
    assert_eq!(app.core.scoped.undo.revision(), revision);
    assert_eq!(app.core.scoped.project_path, previous_path);
    assert!(app.core.scoped.staleness.unsaved_changes);
    std::fs::remove_dir(tmp_path).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn unprepared_reprepared_and_unfinished_models_keep_normal_save_available() {
    for (name, prepare, reprepare, unfinished) in [
        ("unprepared", false, false, false),
        ("reprepared", true, true, false),
        ("unfinished", true, false, true),
    ] {
        let mut app = app(prepare);
        shorten(&mut app);
        if reprepare {
            app.run_preparation();
            assert!(app.core.scoped.last_error.is_none());
        }
        if unfinished {
            assert!(app.apply_model_edit(Box::new(SetElementSection {
                elem: ElemId(2),
                section: None
            })));
        }
        let path = path(name);
        let opened = save_and_open(&mut app, &path);
        assert_eq!(opened.core.model.node(NodeId(3)).unwrap().coord[0], 3000.);
        assert_eq!(
            opened
                .core
                .model
                .element(ElemId(2))
                .unwrap()
                .section
                .is_none(),
            unfinished
        );
        assert_eq!(opened.core.scoped.preparation.is_some(), reprepare);
        if reprepare {
            assert_eq!(opened.core.model.load_cases, app.core.model.load_cases);
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn manual_out_of_range_load_still_rejects_edit_and_save_without_changing_file_or_app() {
    for with_auto in [false, true] {
        let mut app = app(true);
        if !with_auto {
            for case in &mut app.core.model.load_cases {
                case.nodal.clear();
                case.member.clear();
            }
        }
        let path = path("manual-rejection");
        save_and_open(&mut app, &path);
        let bytes = std::fs::read(&path).unwrap();
        let manual = MemberLoad::manual(
            ElemId(2),
            [0., 0., -1.],
            MemberLoadKind::Point { a: 5000., p: 12. },
        );
        assert!(app.apply_model_edit(Box::new(AddMemberLoad {
            lc: LoadCaseId(0),
            load: manual.clone()
        })));
        let before_edit = format!("{:?}", app.core.model);
        assert!(!app.apply_model_edit(Box::new(SetNodeCoord {
            node: NodeId(3),
            coord: [3000., 0., 3500.]
        })));
        assert_eq!(format!("{:?}", app.core.model), before_edit);
        app.undo_action();
        shorten(&mut app);
        app.core.model.load_cases[0].member.push(manual);
        let before_save = format!("{:?}", app.core.model);
        let revision = app.core.scoped.undo.revision();
        let previous_path = app.core.scoped.project_path.clone();
        app.save_project_to(path.clone());
        let error = app.core.scoped.last_error.as_ref().unwrap();
        assert!(error.contains("5000") && error.contains("3000"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(format!("{:?}", app.core.model), before_save);
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert_eq!(app.core.scoped.project_path, previous_path);
        assert!(app.core.scoped.staleness.unsaved_changes);
        assert!(sepika_io::ovika::save_ovika(&path, &app.core.model, Default::default()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn valid_generated_loads_keep_direct_design_input_identical_when_preparation_is_stale() {
    let mut app = super::result_contract_tests::direct_secondary_beam_app();
    app.sync_gravity_load_cases_action();
    assert!(app
        .core
        .model
        .load_cases
        .iter()
        .any(|case| { case.member.iter().any(|load| load.source.is_auto()) }));
    assert!(app.core.model.validate_member_load_extents().is_ok());
    app.core.scoped.staleness.mark_edited();
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(app.core.scoped.staleness.preparation_stale);
    assert!(app.core.scoped.staleness.results_stale);
    assert!(app.require_design_input().is_ok());
    let before = app.current_design_results().unwrap();
    assert!(before.design_source.is_none());
    let input = before.input_records.clone();
    let loads = app.core.model.load_cases.clone();
    let path = path("direct-design");
    let opened = save_and_open(&mut app, &path);
    assert_eq!(opened.core.model.load_cases, loads);
    let design = opened
        .current_design_results()
        .expect("直接検定の生成入力を保持");
    assert_eq!(design.input_records, input);
    assert_eq!((design.beam_checks.len(), design.slab_checks.len()), (1, 2));
    std::fs::remove_file(path).unwrap();
}
