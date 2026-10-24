use super::*;
use sepika_core::model::{
    SecondaryMember, SecondaryMemberAnchor, SecondaryMemberEnds, SecondaryMemberKind,
    SupportMemberId,
};
use sepika_edit::SetBeamVerticalEccentricity;

fn app() -> App {
    let mut app = App::default();
    let mut model = crate::sample::portal_frame();
    let support = SupportMemberId::Primary(ElemId(2));
    model.unassigned_beams.push(SecondaryMember {
        id: sepika_core::ids::SecondaryMemberId(0),
        kind: SecondaryMemberKind::Beam,
        ends: SecondaryMemberEnds::Supported(
            [0.25, 0.75].map(|position| SecondaryMemberAnchor { support, position }),
        ),
        section: Some(SectionId(1)),
        name: "B1".into(),
        gravity_end_shares: None,
        vertical_eccentricity: None,
    });
    assert!(app.load_model(model));
    app
}

#[test]
fn beam_eccentricity_gui_edit_undo_redo_save_reload_and_missing_are_consistent() {
    let mut app = app();
    let primary = SupportMemberId::Primary(ElemId(2));
    let secondary = SupportMemberId::Secondary(sepika_core::ids::SecondaryMemberId(0));
    let nodes = app.core.model.nodes.clone();
    let supports = app.core.model.unassigned_beams[0].ends;
    app.core.scoped.staleness.mark_fresh();
    app.core.scoped.staleness.preparation_stale = false;
    app.core.scoped.staleness.design_stale = false;
    let input = app.result_input(&result_validity::ResultInputKey::Modal);
    assert!(app.apply_model_edit(Box::new(SetBeamVerticalEccentricity {
        member: primary,
        value: Some(100.0)
    })));
    assert!(app.core.scoped.staleness.results_stale);
    assert!(app.core.scoped.staleness.preparation_stale);
    assert!(app.core.scoped.staleness.design_stale);
    assert_ne!(
        app.result_input(&result_validity::ResultInputKey::Modal),
        input
    );
    assert_eq!(
        app.core.model.beam_placement(primary).unwrap().actual_ends[0][2],
        3600.0
    );
    assert!(app.core.model.beam_placement(secondary).is_err());
    assert!(app.apply_model_edit(Box::new(SetBeamVerticalEccentricity {
        member: secondary,
        value: Some(-100.0)
    })));
    assert_eq!(
        app.core
            .model
            .beam_placement(secondary)
            .unwrap()
            .actual_ends[0][2],
        3400.0
    );
    app.undo_action();
    assert_eq!(
        app.core.model.unassigned_beams[0].vertical_eccentricity,
        None
    );
    app.redo_action();
    assert_eq!(
        app.core.model.unassigned_beams[0].vertical_eccentricity,
        Some(-100.0)
    );
    assert_eq!(app.core.model.nodes, nodes);
    assert_eq!(app.core.model.unassigned_beams[0].ends, supports);
    let path = std::env::temp_dir().join(format!("sepika-726-{}.ovika", std::process::id()));
    for value in [Some(-100.0), Some(0.0), None] {
        app.core.model.unassigned_beams[0].vertical_eccentricity = value;
        sepika_io::ovika::save_ovika(&path, &app.core.model, Default::default()).unwrap();
        let restored = sepika_io::ovika::load_ovika(&path).unwrap().model;
        assert_eq!(restored.unassigned_beams[0].vertical_eccentricity, value);
        assert_eq!(restored.elements[2].vertical_eccentricity, Some(100.0));
        assert_eq!(restored.unassigned_beams[0].ends, supports);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn beam_placement_does_not_transform_existing_analysis_stiffness_or_connectivity() {
    let mut model = crate::sample::portal_frame();
    let before = sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(0)).unwrap();
    let nodes = model.nodes.clone();
    let connectivity = model
        .elements
        .iter()
        .map(|e| (e.id, e.nodes.clone(), e.end_cond, e.rigid_zone))
        .collect::<Vec<_>>();
    model.elements[2].vertical_eccentricity = Some(750.0);
    let after = sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(0)).unwrap();
    assert_eq!(
        rmp_serde::to_vec_named(&before).unwrap(),
        rmp_serde::to_vec_named(&after).unwrap()
    );
    assert_eq!(model.nodes, nodes);
    assert_eq!(
        model
            .elements
            .iter()
            .map(|e| (e.id, e.nodes.clone(), e.end_cond, e.rigid_zone))
            .collect::<Vec<_>>(),
        connectivity
    );
}

#[test]
fn node_and_section_edits_invalidate_results_and_update_placement() {
    let mut app = app();
    let member = SupportMemberId::Primary(ElemId(2));
    assert!(app.apply_model_edit(Box::new(SetBeamVerticalEccentricity {
        member,
        value: Some(100.0)
    })));
    let initial = app.core.model.beam_placement(member).unwrap().actual_ends;
    assert!(app.apply_model_edit(Box::new(sepika_edit::SetNodeCoord {
        node: NodeId(2),
        coord: [0.0, 0.0, 3600.0]
    })));
    assert_eq!(
        app.core.model.beam_placement(member).unwrap().actual_ends[0][2],
        3700.0
    );
    assert_eq!(
        app.core.model.elements[2].vertical_eccentricity,
        Some(100.0)
    );
    assert!(app.core.scoped.staleness.results_stale);
    app.undo_action();
    assert_eq!(
        app.core.model.beam_placement(member).unwrap().actual_ends,
        initial
    );
    let old_top = app
        .core
        .model
        .beam_placement(member)
        .unwrap()
        .vertical_range([3000.0, 0.0])
        .unwrap()
        .unwrap()[1];
    let mut shape = app.core.model.sections[1].shape.clone().unwrap();
    if let sepika_core::section_shape::SectionShape::SteelH { height, .. } = &mut shape {
        *height += 100.0;
    }
    assert!(
        app.apply_model_edit(Box::new(sepika_edit::EditSectionShape {
            section: SectionId(1),
            new_shape: shape,
            frame_use: Some(sepika_core::model::FrameSectionUse::Girder)
        }))
    );
    let placement = app.core.model.beam_placement(member).unwrap();
    assert_eq!(placement.actual_ends, initial);
    assert_eq!(
        placement.vertical_range([3000.0, 0.0]).unwrap().unwrap()[1],
        old_top + 50.0
    );
    assert!(app.core.scoped.staleness.results_stale);
    assert!(app.apply_model_edit(Box::new(SetBeamVerticalEccentricity {
        member,
        value: None
    })));
    let revision = app.core.scoped.undo.revision();
    assert!(!app.apply_model_edit(Box::new(SetBeamVerticalEccentricity {
        member,
        value: Some(f64::INFINITY)
    })));
    assert_eq!(app.core.scoped.undo.revision(), revision);
    assert!(app.core.model.beam_placement(member).is_err());
}
