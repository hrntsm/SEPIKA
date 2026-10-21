use super::*;
use sepika_core::ids::{StoryId, WallPlateAssignmentRegionId, WallPlateId};
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

const BUILT_BOX_INPUT: &str =
    include_str!("../../../sepika-io/tests/fixtures/built_box_standard.stb");

#[test]
fn built_box_equal_thickness_import_replaces_model_only_after_success() {
    for t2 in ["9", "9.0"] {
        let mut app = existing_app();
        let source = BUILT_BOX_INPUT.replace("t2=\"9\"", &format!("t2=\"{t2}\""));
        let file = path(&format!("742-equal-{t2}")).with_extension("stb");
        std::fs::write(&file, &source).unwrap();

        app.import_stbridge_from(file.clone());

        assert_eq!(app.core.model.elements.len(), 1);
        assert_eq!(app.core.model.sections.len(), 1);
        let section = &app.core.model.sections[0];
        assert_eq!(section.name, "G1");
        assert_eq!(
            section.shape,
            Some(sepika_core::section_shape::SectionShape::SteelBox {
                height: 300.,
                width: 200.,
                thick: 9.,
                corner_r: Some(0.),
            })
        );
        assert!(app.core.scoped.results.is_none());
        assert!(app.core.scoped.preparation.is_none());
        assert!(app.core.scoped.project_path.is_none());
        assert!(!app.core.scoped.undo.can_undo());
        assert!(!app.core.scoped.undo.can_redo());
        assert!(!app
            .core
            .log
            .entries
            .iter()
            .any(|entry| entry.message.contains("ST-Bridge読込エラー")));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), source);
        std::fs::remove_file(file).unwrap();
    }
}

#[cfg(feature = "gui")]
fn assert_built_box_error_is_displayed(app: &mut App, error: &str) {
    assert!(app.ui.view.bottom_dock_open);
    assert_eq!(app.ui.view.bottom_tab, BottomTab::Log);
    let output = egui::Context::default().run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(2400., 800.),
            )),
            ..Default::default()
        },
        |ui| app.status_bar(ui),
    );
    let expected = format!("⚠ {}", error.replace('\n', " "));
    assert!(output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == expected)
    }));
}

#[test]
fn built_box_rejected_import_preserves_model_results_history_and_files_with_diagnostic() {
    let mut app = existing_app();
    let project = path("742-retained-project");
    sepika_io::ovika::save_ovika(&project, &app.core.model, Default::default()).unwrap();
    app.core.scoped.project_path = Some(project.clone());
    let project_bytes = std::fs::read(&project).unwrap();
    let before = retained(&app);

    for (label, replacement, details) in [
        (
            "unequal-6",
            "t2=\"6\"",
            &["t1", "9", "t2", "6", "未対応"][..],
        ),
        (
            "unequal-12",
            "t2=\"12\"",
            &["t1", "9", "t2", "12", "未対応"][..],
        ),
        ("missing", "", &["t2", "欠落"][..]),
        ("zero", "t2=\"0\"", &["t2", "0", "正値"][..]),
        ("negative", "t2=\"-1\"", &["t2", "-1", "正値"][..]),
        ("nonnumeric", "t2=\"bad\"", &["t2", "bad", "数値"][..]),
        ("nan", "t2=\"NaN\"", &["t2", "NaN", "有限"][..]),
        ("infinite", "t2=\"INF\"", &["t2", "INF", "有限"][..]),
        ("inner-height", "t2=\"151\"", &["t2", "151", "内法"][..]),
    ] {
        let source = BUILT_BOX_INPUT.replace("t2=\"9\"", replacement);
        let file = path(&format!("742-rejected-{label}")).with_extension("stb");
        std::fs::write(&file, &source).unwrap();
        #[cfg(feature = "gui")]
        {
            app.ui.view.bottom_dock_open = false;
            app.ui.view.bottom_tab = BottomTab::Model;
        }

        app.import_stbridge_from(file.clone());

        let error = app.core.scoped.last_error.clone().unwrap();
        for detail in [
            "ST-Bridge読込エラー",
            "StbSecBuild-BOX",
            "BOX1",
            "StbSecBeam_S",
            "20",
            "G1",
        ]
        .into_iter()
        .chain(details.iter().copied())
        {
            assert!(error.contains(detail), "{label}/{detail}: {error}");
        }
        let entry = app.core.log.entries.last().unwrap();
        assert_eq!(entry.level, LogLevel::Error);
        assert_eq!(entry.message, error);
        #[cfg(feature = "gui")]
        assert_built_box_error_is_displayed(&mut app, &error);
        assert_eq!(retained(&app), before, "{label}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), source);
        assert_eq!(std::fs::read(&project).unwrap(), project_bytes);
        std::fs::remove_file(file).unwrap();
    }
    app.core.scoped.undo.undo(&mut app.core.model);
    assert_eq!(app.core.model.nodes[0].coord[0], 0.);
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_eq!(bytes(&app.core.model), before.model);
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_eq!(app.core.model.nodes[0].coord[0], -200.);
    std::fs::remove_file(project).unwrap();
}

const SERIES_COLUMN: &str = r#"<StbSecColumn_RC id="1" name="C1" strength_concrete="Fc30"><StbSecFigureColumn_RC><StbSecColumn_RC_Rect width_X="500" width_Y="500"/></StbSecFigureColumn_RC></StbSecColumn_RC>"#;
const SERIES_GIRDER: &str = r#"<StbSecBeam_RC id="1" name="G1" strength_concrete="Fc24"><StbSecFigureBeam_RC><StbSecBeam_RC_Straight width="300" depth="600"/></StbSecFigureBeam_RC></StbSecBeam_RC>"#;

fn independent_series_input() -> String {
    format!(
        r#"<ST_BRIDGE xmlns="https://www.building-smart.or.jp/dl" version="2.0.2">
<StbCommon project_name="independent" app_name="fixture" app_version="1"/>
<StbModel><StbNodes>
<StbNode id="10" X="0" Y="0" Z="0" kind="ON_GRID"/>
<StbNode id="20" X="0" Y="0" Z="3000" kind="ON_GRID"/>
<StbNode id="30" X="6000" Y="0" Z="3000" kind="ON_GRID"/>
</StbNodes><StbMembers>
<StbColumns><StbColumn id="1" name="C1" id_node_bottom="10" id_node_top="20" id_section="1" kind_structure="RC"/></StbColumns>
<StbGirders><StbGirder id="1" name="G1" id_node_start="20" id_node_end="30" id_section="1" kind_structure="RC" isFoundation="false"/></StbGirders>
</StbMembers><StbSections>{SERIES_COLUMN}{SERIES_GIRDER}</StbSections></StbModel></ST_BRIDGE>"#
    )
}

#[test]
fn qualified_series_import_failures_preserve_app_state_history_and_files() {
    let source = independent_series_input();
    let mut cases: Vec<(&str, String, &[&str])> = vec![
        (
            "missing-column-family",
            source.replace(SERIES_COLUMN, ""),
            &[
                "StbColumn",
                "id=Some(1)",
                "RC",
                "StbSecColumn_RC",
                "id_section=Some(\"1\")",
                "断面参照",
            ],
        ),
        (
            "wrong-namespace",
            source.replace("https://www.building-smart.or.jp/dl", "urn:foreign"),
            &["ST_BRIDGE", "非採用 namespace", "urn:foreign"],
        ),
        (
            "no-namespace",
            source.replace(" xmlns=\"https://www.building-smart.or.jp/dl\"", ""),
            &["ST_BRIDGE", "namespace がありません"],
        ),
        (
            "nested-wrong-namespace",
            source.replace("<StbNodes>", "<StbNodes xmlns=\"urn:foreign\">"),
            &["StbNodes", "非採用 namespace", "urn:foreign"],
        ),
        (
            "unbound-element-prefix",
            source.replace("<StbNode ", "<missing:StbNode "),
            &["missing:StbNode", "未束縛 prefix", "missing"],
        ),
        (
            "unbound-attribute-prefix",
            source.replacen("id_section=\"1\"", "missing:id_section=\"1\"", 1),
            &["StbColumn", "missing:id_section", "未束縛 prefix"],
        ),
    ];
    for (label, id) in [("duplicate-1", "1"), ("duplicate-01", "01")] {
        let duplicate = SERIES_COLUMN.replace("id=\"1\"", &format!("id=\"{id}\""));
        cases.push((
            label,
            source.replace(SERIES_COLUMN, &format!("{SERIES_COLUMN}{duplicate}")),
            &["StbSecColumn_RC", "1", "重複"],
        ));
    }
    for (label, replacement, details) in [
        (
            "missing-kind",
            "",
            &[
                "StbColumn",
                "id=Some(1)",
                "id_section=Some(\"1\")",
                "kind_structure",
                "ありません",
            ][..],
        ),
        (
            "invalid-kind",
            "kind_structure=\"INVALID\"",
            &[
                "StbColumn",
                "id=Some(1)",
                "id_section=Some(\"1\")",
                "INVALID",
                "不正",
            ][..],
        ),
        (
            "unsupported-kind",
            "kind_structure=\"UNDEFINED\"",
            &[
                "StbColumn",
                "id=Some(1)",
                "id_section=Some(\"1\")",
                "UNDEFINED",
                "未対応",
            ][..],
        ),
    ] {
        let mut xml = source.replacen("kind_structure=\"RC\"", replacement, 1);
        if label == "unsupported-kind" {
            xml = xml.replace(
                "</StbSections>",
                r#"<StbSecUndefined id="1" name="UNDEFINED1"/></StbSections>"#,
            );
        }
        cases.push((label, xml, details));
    }
    let mut app = existing_app();
    let project = path("571-retained-project");
    sepika_io::ovika::save_ovika(&project, &app.core.model, Default::default()).unwrap();
    app.core.scoped.project_path = Some(project.clone());
    let project_bytes = std::fs::read(&project).unwrap();
    let before = retained(&app);
    for (label, xml, details) in cases {
        let file = path(&format!("571-{label}")).with_extension("stb");
        std::fs::write(&file, &xml).unwrap();
        app.import_stbridge_from(file.clone());
        let error = app.core.scoped.last_error.as_ref().unwrap();
        assert!(error.contains("ST-Bridge読込エラー"), "{label}: {error}");
        for detail in details {
            assert!(error.contains(detail), "{label}/{detail}: {error}");
        }
        assert_eq!(retained(&app), before, "{label}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), xml);
        assert_eq!(std::fs::read(&project).unwrap(), project_bytes);
        std::fs::remove_file(file).unwrap();
    }
    app.core.scoped.undo.undo(&mut app.core.model);
    assert_eq!(app.core.model.nodes[0].coord[0], 0.);
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_eq!(bytes(&app.core.model), before.model);
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_eq!(app.core.model.nodes[0].coord[0], -200.);
    std::fs::remove_file(project).unwrap();
}

#[test]
fn qualified_series_app_import_binds_column_and_girder_to_their_own_fc() {
    let source = independent_series_input();
    let file = path("571-independent-series").with_extension("stb");
    std::fs::write(&file, &source).unwrap();
    let mut app = App::default();
    app.import_stbridge_from(file.clone());
    assert_eq!(app.core.model.elements.len(), 2);
    let mut section_ids = Vec::new();
    for (usage, name, fc) in [
        (FrameSectionUse::Column, "C1", 30.),
        (FrameSectionUse::Girder, "G1", 24.),
    ] {
        let element = app
            .core
            .model
            .elements
            .iter()
            .find(|element| {
                app.core
                    .model
                    .section(element.section.unwrap())
                    .unwrap()
                    .frame_use
                    == Some(usage)
            })
            .unwrap();
        let section = app.core.model.section(element.section.unwrap()).unwrap();
        assert_eq!(section.name, name);
        assert_eq!(
            app.core.model.element_material(element).unwrap().fc,
            Some(fc)
        );
        let strength = app
            .core
            .model
            .stb_strengths
            .members
            .iter()
            .find(|strength| strength.target == StrengthTarget::Element(element.id))
            .unwrap();
        let resolved = app.core.model.resolve_stb_concrete(strength).unwrap();
        assert_eq!(resolved.value, fc);
        assert_eq!(resolved.source, StrengthSource::Section);
        section_ids.push(section.id);
    }
    assert_ne!(section_ids[0], section_ids[1]);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), source);
    std::fs::remove_file(file).unwrap();
}

#[test]
fn standard_stbridge_bad_references_preserve_app_model_history_results_and_generation() {
    let source = include_str!("../../../sepika-io/tests/fixtures/strength_src.stb");
    assert!(sepika_io::stbridge::import_stbridge(source).is_ok());
    let mut app = existing_app();
    let before = retained(&app);
    for (label, original, invalid, reference) in [
        ("node", "id_node_top=\"2\"", "id_node_top=\"99\"", "99"),
        ("section", "id_section=\"1\"", "id_section=\"99\"", "99"),
        ("steel", "shape=\"H1\"", "shape=\"MISSING\"", "MISSING"),
    ] {
        let file = path(&format!("stbridge660-{label}")).with_extension("stb");
        let xml = source.replace(original, invalid);
        std::fs::write(&file, &xml).unwrap();
        app.import_stbridge_from(file.clone());
        let reason = app.core.scoped.last_error.as_ref().unwrap();
        assert!(
            reason.contains("ST-Bridge読込エラー") && reason.contains(reference),
            "{reason}"
        );
        assert_eq!(retained(&app), before, "{label}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), xml);
        std::fs::remove_file(file).unwrap();
    }
    app.core.scoped.undo.undo(&mut app.core.model);
    assert_eq!(app.core.model.nodes[0].coord[0], 0.);
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_eq!(bytes(&app.core.model), before.model);
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_eq!(app.core.model.nodes[0].coord[0], -200.);
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

#[derive(Clone, Copy, Debug)]
enum InternalReference {
    NodeStory,
    StoryNode,
    DiaphragmStory,
    DiaphragmMaster,
    DiaphragmSlave,
    LinkMaster,
    LinkSlave,
    MpcMaster,
    MpcTerm,
    ManualLoad,
    AutoLoad,
    GeneratedMaster,
    CalculatedStory,
    RecordedDiaphragmStory,
    RecordedDiaphragmMaster,
    RecordedDiaphragmSlave,
    RecordedLinkMaster,
    RecordedLinkSlave,
    RecordedMpcMaster,
    RecordedMpcTerm,
    RecordedRestraint,
}

impl InternalReference {
    const ALL: [Self; 21] = [
        Self::NodeStory,
        Self::StoryNode,
        Self::DiaphragmStory,
        Self::DiaphragmMaster,
        Self::DiaphragmSlave,
        Self::LinkMaster,
        Self::LinkSlave,
        Self::MpcMaster,
        Self::MpcTerm,
        Self::ManualLoad,
        Self::AutoLoad,
        Self::GeneratedMaster,
        Self::CalculatedStory,
        Self::RecordedDiaphragmStory,
        Self::RecordedDiaphragmMaster,
        Self::RecordedDiaphragmSlave,
        Self::RecordedLinkMaster,
        Self::RecordedLinkSlave,
        Self::RecordedMpcMaster,
        Self::RecordedMpcTerm,
        Self::RecordedRestraint,
    ];

    fn target_kind(self) -> &'static str {
        match self {
            Self::NodeStory
            | Self::DiaphragmStory
            | Self::CalculatedStory
            | Self::RecordedDiaphragmStory => "Story",
            _ => "Node",
        }
    }

    fn model(self, mut model: Model, absent: Option<u32>) -> Model {
        use sepika_core::dof::{Dof, Dof6Mask};
        model.stories.push(Story {
            id: StoryId(0),
            name: "基部".into(),
            elevation: 0.,
            node_ids: vec![],
            seismic_weight: None,
            weight_override: None,
            dynamic_mass: None,
            standard_floor_load: None,
            column_finish_area_weight: 0.,
            fireproof: Default::default(),
            wall_weights: vec![],
            level_kind: Default::default(),
            structure: Default::default(),
        });
        let master = NodeId(absent.unwrap_or(0));
        let slave = NodeId(absent.unwrap_or(1));
        let story = StoryId(absent.unwrap_or(0));
        let mut record = SeismicWeightGeneration {
            input_key: vec![],
            output_key: vec![],
            calculated_weights: vec![],
            automatic_diaphragms: vec![],
            automatic_master_restraints: vec![],
        };
        match self {
            Self::NodeStory => model.nodes[0].story = Some(story),
            Self::StoryNode => model.stories[0].node_ids = vec![master],
            Self::DiaphragmStory => model.constraints.push(Constraint::rigid_diaphragm(
                story,
                NodeId(0),
                vec![NodeId(1)],
            )),
            Self::DiaphragmMaster => model.constraints.push(Constraint::rigid_diaphragm(
                StoryId(0),
                master,
                vec![NodeId(1)],
            )),
            Self::DiaphragmSlave => model.constraints.push(Constraint::rigid_diaphragm(
                StoryId(0),
                NodeId(0),
                vec![slave],
            )),
            Self::LinkMaster | Self::LinkSlave => model.constraints.push(Constraint::RigidLink {
                master: if matches!(self, Self::LinkMaster) {
                    master
                } else {
                    NodeId(0)
                },
                slaves: vec![if matches!(self, Self::LinkSlave) {
                    slave
                } else {
                    NodeId(1)
                }],
                dofs: Dof6Mask::FIXED,
            }),
            Self::MpcMaster | Self::MpcTerm => model.constraints.push(Constraint::Mpc {
                master: if matches!(self, Self::MpcMaster) {
                    master
                } else {
                    NodeId(0)
                },
                terms: vec![(
                    if matches!(self, Self::MpcTerm) {
                        slave
                    } else {
                        NodeId(1)
                    },
                    Dof::Ux,
                    1.,
                )],
            }),
            Self::ManualLoad => model.load_cases[0]
                .nodal
                .push(NodalLoad::manual(master, [1.; 6])),
            Self::AutoLoad => model.load_cases[0]
                .nodal
                .push(NodalLoad::auto(master, [1.; 6])),
            Self::GeneratedMaster => model.generated_masters = vec![master],
            Self::CalculatedStory => record.calculated_weights = vec![(story, 100.)],
            Self::RecordedDiaphragmStory => {
                record
                    .automatic_diaphragms
                    .push(Constraint::rigid_diaphragm(
                        story,
                        NodeId(0),
                        vec![NodeId(1)],
                    ))
            }
            Self::RecordedDiaphragmMaster => {
                record
                    .automatic_diaphragms
                    .push(Constraint::rigid_diaphragm(
                        StoryId(0),
                        master,
                        vec![NodeId(1)],
                    ))
            }
            Self::RecordedDiaphragmSlave => {
                record
                    .automatic_diaphragms
                    .push(Constraint::rigid_diaphragm(
                        StoryId(0),
                        NodeId(0),
                        vec![slave],
                    ))
            }
            Self::RecordedLinkMaster | Self::RecordedLinkSlave => {
                record.automatic_diaphragms.push(Constraint::RigidLink {
                    master: if matches!(self, Self::RecordedLinkMaster) {
                        master
                    } else {
                        NodeId(0)
                    },
                    slaves: vec![if matches!(self, Self::RecordedLinkSlave) {
                        slave
                    } else {
                        NodeId(1)
                    }],
                    dofs: Dof6Mask::FIXED,
                })
            }
            Self::RecordedMpcMaster | Self::RecordedMpcTerm => {
                record.automatic_diaphragms.push(Constraint::Mpc {
                    master: if matches!(self, Self::RecordedMpcMaster) {
                        master
                    } else {
                        NodeId(0)
                    },
                    terms: vec![(
                        if matches!(self, Self::RecordedMpcTerm) {
                            slave
                        } else {
                            NodeId(1)
                        },
                        Dof::Ux,
                        1.,
                    )],
                })
            }
            Self::RecordedRestraint => {
                record.automatic_master_restraints = vec![(master, Dof6Mask::FIXED)]
            }
        }
        if matches!(
            self,
            Self::CalculatedStory
                | Self::RecordedDiaphragmStory
                | Self::RecordedDiaphragmMaster
                | Self::RecordedDiaphragmSlave
                | Self::RecordedLinkMaster
                | Self::RecordedLinkSlave
                | Self::RecordedMpcMaster
                | Self::RecordedMpcTerm
                | Self::RecordedRestraint
        ) {
            model.seismic_weight_generation = Some(record);
        }
        model
    }
}

#[test]
fn internal_reference_app_entries_accept_existing_targets_and_preserve_rejected_state_and_files() {
    for reference in InternalReference::ALL {
        let normal = reference.model(crate::sample::portal_frame(), None);
        let mut control = App::default();
        assert!(
            control.load_model(normal.clone()),
            "{reference:?}: {:?}",
            control.core.scoped.last_error
        );
        let normal_file = path(&format!("681-normal-{reference:?}"));
        control.save_project_to_opts(normal_file.clone(), Some(false));
        assert!(
            control.core.scoped.last_error.is_none(),
            "{reference:?}: {:?}",
            control.core.scoped.last_error
        );
        control.open_project_from(normal_file.clone());
        assert!(
            control.core.scoped.last_error.is_none(),
            "{reference:?}: {:?}",
            control.core.scoped.last_error
        );
        assert_eq!(bytes(&control.core.model), bytes(&normal), "{reference:?}");
        std::fs::remove_file(normal_file).unwrap();

        let next_absent = if reference.target_kind() == "Story" {
            1
        } else {
            4
        };
        for absent in [next_absent, u32::MAX] {
            let invalid = reference.model(crate::sample::portal_frame(), Some(absent));
            let input = path(&format!("681-input-{reference:?}-{absent}"));
            super::secondary_cft_tests::write_invalid_container(&input, &invalid);
            let input_bytes = std::fs::read(&input).unwrap();
            let output = path(&format!("681-output-{reference:?}-{absent}"));
            sepika_io::ovika::save_ovika(&output, &normal, Default::default()).unwrap();
            let output_bytes = std::fs::read(&output).unwrap();
            let mut app = existing_app();
            let before_load = retained(&app);
            for open_file in [false, true] {
                if open_file {
                    app.open_project_from(input.clone());
                } else {
                    assert!(!app.load_model(invalid.clone()), "{reference:?}/{absent}");
                }
                let error = app.core.scoped.last_error.as_ref().unwrap();
                assert!(
                    error.contains(&format!("-> {} {absent}", reference.target_kind())),
                    "{reference:?}: {error}"
                );
                assert_eq!(retained(&app), before_load, "{reference:?}/{absent}");
                assert_eq!(std::fs::read(&input).unwrap(), input_bytes);
                assert_eq!(std::fs::read(&output).unwrap(), output_bytes);
            }

            app.core.model = reference.model(app.core.model.clone(), Some(absent));
            let bundle = app.core.scoped.results.as_mut().unwrap();
            bundle.pushover = Some(pushover(11.));
            bundle.pushover_y = Some(pushover(33.));
            app.core.scoped.pushover_view_dir = SeismicDir::Y;
            let before_save = retained(&app);
            app.save_project_to_opts(output.clone(), Some(false));
            let error = app.core.scoped.last_error.as_ref().unwrap();
            assert!(
                error.contains(&format!("-> {} {absent}", reference.target_kind())),
                "{reference:?}: {error}"
            );
            assert_eq!(retained(&app), before_save, "{reference:?}/{absent}");
            assert_eq!(std::fs::read(&input).unwrap(), input_bytes);
            assert_eq!(std::fs::read(&output).unwrap(), output_bytes);

            app.core.scoped.undo.undo(&mut app.core.model);
            assert_eq!(app.core.model.nodes[0].coord[0], 0.);
            app.core.scoped.undo.redo(&mut app.core.model);
            assert_eq!(bytes(&app.core.model), before_save.model);
            app.core.scoped.undo.redo(&mut app.core.model);
            assert_eq!(app.core.model.nodes[0].coord[0], -200.);
            std::fs::remove_file(input).unwrap();
            std::fs::remove_file(output).unwrap();
        }
    }
}

#[test]
fn internal_reference_app_roundtrip_keeps_stale_generation_and_inactive_owner() {
    use sepika_core::dof::Dof6Mask;
    let mut model = InternalReference::CalculatedStory.model(crate::sample::portal_frame(), None);
    let owner = NodeId(model.nodes.len() as u32);
    model.nodes.push(Node {
        id: owner,
        coord: [0., 0., 3500.],
        restraint: Dof6Mask::FIXED,
        mass: None,
        story: None,
        support_spring: None,
    });
    model.generated_masters = vec![owner];
    model.stories[0].seismic_weight = Some(200.);
    model.constraints.push(Constraint::rigid_diaphragm(
        StoryId(0),
        NodeId(0),
        vec![NodeId(1)],
    ));
    let record = model.seismic_weight_generation.as_mut().unwrap();
    record.input_key = vec![255];
    record.output_key = vec![254];
    record.automatic_diaphragms = vec![Constraint::rigid_diaphragm(
        StoryId(0),
        owner,
        vec![NodeId(1)],
    )];
    record.automatic_master_restraints = vec![(owner, Dof6Mask::FREE)];
    assert!(!model.is_automatic_seismic_diaphragm(&model.constraints[0]));
    assert!(!model.is_automatic_seismic_master_restraint(owner));
    let expected = bytes(&model);
    let mut app = App::default();
    assert!(app.load_model(model));
    let file = path("681-stale-record-inactive-owner");
    app.save_project_to_opts(file.clone(), Some(false));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert_eq!(bytes(&app.core.model), expected);
    let mut reopened = existing_app();
    reopened.open_project_from(file.clone());
    assert!(
        reopened.core.scoped.last_error.is_none(),
        "{:?}",
        reopened.core.scoped.last_error
    );
    assert_eq!(bytes(&reopened.core.model), expected);
    std::fs::remove_file(file).unwrap();
}
