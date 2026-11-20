use sepika_app::app::references::{DisplayConditions, ReferenceTarget, ResultInputKey};
use sepika_app::app::{App, StaticCaseKey};
use sepika_app::sample;
use sepika_core::ids::{ElemId, FloorPlateAssignmentRegionId, SlabId};
use sepika_core::model::{
    ElementKind, FloorPlateAssignmentRegion, PlateAssignment, Slab, SlabShape, SupportBoundary,
    SupportMemberId,
};

#[test]
fn panel_removal_diagnostic_preserves_input_and_rejects_current_use_of_old_result() {
    let mut app = App::default();
    assert!(app.load_model(sample::portal_frame()));
    let case = app.core.model.load_cases[0].id;
    app.run_linear_static(case);
    assert_eq!(app.core.scoped.last_error, None);
    let valid_connection = app.connection_reference();
    let old_result = app
        .result_reference(
            app.target_reference(ReferenceTarget::Member(ElemId(0)))
                .unwrap(),
            ResultInputKey::Static(StaticCaseKey::User(case)),
            DisplayConditions::default(),
        )
        .unwrap();
    assert!(app.resolve_result_reference(&old_result).is_ok());

    let panel_id = app
        .core
        .model
        .elements
        .iter()
        .find(|element| element.kind == ElementKind::PanelZone)
        .unwrap()
        .id;
    app.core
        .model
        .load_cfg
        .get_or_insert_with(Default::default)
        .extra_line_weight
        .push((panel_id, 0.5));
    let before = bincode::serialize(&app.core.model).unwrap();

    let invalid_input = app.input_reference();
    let invalid_connection = app.connection_reference();
    assert_eq!(invalid_connection.input, invalid_input);
    assert_ne!(invalid_input.input, valid_connection.input.input);
    assert!(app
        .validate_connection_reference(&valid_connection)
        .is_err());
    assert!(app.resolve_result_reference(&old_result).is_err());

    let mut without_panels = App::default();
    without_panels.core.model = app.core.model.clone();
    without_panels
        .core
        .model
        .elements
        .retain(|element| element.kind != ElementKind::PanelZone);
    assert_ne!(without_panels.input_reference().input, invalid_input.input);

    app.run_preparation();
    let diagnostic = app.core.scoped.last_error.as_deref().unwrap();
    assert!(diagnostic.contains("部材付加線重量"), "{diagnostic}");
    assert!(
        diagnostic.contains(&format!("削除対象の仕口パネル要素 {}", panel_id.0)),
        "{diagnostic}"
    );
    assert_eq!(bincode::serialize(&app.core.model).unwrap(), before);
    assert_eq!(app.input_reference(), invalid_input);
    assert!(app
        .validate_connection_reference(&invalid_connection)
        .is_ok());
}

#[test]
fn preparation_reports_which_plate_assignment_references_a_panel_before_rebuilding_regions() {
    let mut app = App::default();
    assert!(app.load_model(sample::portal_frame()));
    app.run_preparation();
    assert_eq!(app.core.scoped.last_error, None);
    let model = &mut app.core.model;
    let panel = model
        .elements
        .iter()
        .find(|e| e.kind == ElementKind::PanelZone)
        .unwrap()
        .id;
    model.slabs.push(Slab {
        id: SlabId(0),
        shape: SlabShape::Enclosed,
        plate: Default::default(),
        reference_story: None,
        vertical_eccentricity_mm: None,
        tip_loads: vec![],
    });
    model
        .floor_assignment_regions
        .regions
        .push(FloorPlateAssignmentRegion {
            id: FloorPlateAssignmentRegionId(17),
            boundary: vec![SupportBoundary {
                support: SupportMemberId::Primary(panel),
                span: [0., 1.],
            }],
            assignment: PlateAssignment::Plate(SlabId(0)),
        });
    let before = bincode::serialize(&app.core.model).unwrap();

    app.run_preparation();

    let diagnostic = app.core.scoped.last_error.as_deref().unwrap();
    assert!(diagnostic.contains("床板割当領域 ID 17"), "{diagnostic}");
    assert!(
        diagnostic.contains(&format!("削除対象の仕口パネル要素 {}", panel.0)),
        "{diagnostic}"
    );
    assert_eq!(bincode::serialize(&app.core.model).unwrap(), before);
}
