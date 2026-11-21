use super::*;
use sepika_core::model::{
    SecondaryMember, SecondaryMemberAnchor, SecondaryMemberEnds, SecondaryMemberKind,
    SupportMemberId,
};
use sepika_edit::SetMemberEndOffsets;

fn app() -> App {
    let mut app = App::default();
    let mut model = crate::sample::portal_frame();
    let support = SupportMemberId::Primary(ElemId(2));
    model.unassigned_beams.push(SecondaryMember {
        display_local_axis: None,
        id: sepika_core::ids::SecondaryMemberId(0),
        kind: SecondaryMemberKind::Beam,
        ends: SecondaryMemberEnds::Supported(
            [0.25, 0.75].map(|position| SecondaryMemberAnchor { support, position }),
        ),
        section: Some(SectionId(1)),
        name: "B1".into(),
        gravity_end_shares: None,
        end_offsets_mm: [[0.0; 3]; 2],
    });
    assert!(app.load_model(model));
    app
}

#[test]
fn changing_cantilever_support_preserves_asymmetric_display_and_end_offsets_through_undo_and_save()
{
    let mut app = app();
    let section_id = SectionId(app.core.model.sections.len() as u32);
    let section = sepika_core::section_shape::SectionShape::SteelAngle {
        leg_a: 100.0,
        leg_b: 80.0,
        thick: 10.0,
    }
    .to_section(section_id, "表示用L".into());
    app.core.model.sections.push(section);
    app.core.model.unassigned_beams[0].section = Some(section_id);
    app.core.model.unassigned_beams[0].end_offsets_mm = [[10.0, 20.0, 30.0], [-40.0, 20.0, 30.0]];
    let member_id = app.core.model.unassigned_beams[0].id;
    let target = SupportMemberId::Secondary(member_id);
    let original = app.core.model.beam_placement(target).unwrap();
    let original_member = app.core.model.unassigned_beams[0].clone();
    assert!(
        app.apply_model_edit(Box::new(sepika_edit::SetSecondaryMemberEndSupport {
            member: member_id,
            end_support: [
                sepika_core::model::EndSupport::Free,
                sepika_core::model::EndSupport::Supported
            ],
        }))
    );
    let reversed = app.core.model.beam_placement(target).unwrap();
    assert_eq!(
        reversed.actual_ends,
        [original.actual_ends[1], original.actual_ends[0]]
    );
    assert_eq!(reversed.axes[1], original.axes[1]);
    assert_eq!(reversed.axes[2], original.axes[2]);
    app.undo_action();
    assert_eq!(app.core.model.unassigned_beams[0], original_member);
    app.redo_action();
    let path =
        std::env::temp_dir().join(format!("sepika-display-1367-{}.ovika", std::process::id()));
    sepika_io::ovika::save_ovika(&path, &app.core.model, Default::default()).unwrap();
    let loaded = sepika_io::ovika::load_ovika(&path).unwrap().model;
    std::fs::remove_file(path).unwrap();
    assert_eq!(loaded.beam_placement(target).unwrap().axes, reversed.axes);
    assert_eq!(
        loaded.beam_placement(target).unwrap().actual_ends,
        reversed.actual_ends
    );
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
    assert!(app.apply_model_edit(Box::new(SetMemberEndOffsets {
        member: primary,
        value: [[0.0, 0.0, 100.0]; 2]
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
    assert!(app.core.model.beam_placement(secondary).is_ok());
    assert!(app.apply_model_edit(Box::new(SetMemberEndOffsets {
        member: secondary,
        value: [[0.0, 0.0, -100.0]; 2]
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
        app.core.model.unassigned_beams[0].end_offsets_mm,
        [[0.0; 3]; 2]
    );
    app.redo_action();
    assert_eq!(
        app.core.model.unassigned_beams[0].end_offsets_mm,
        [[0.0, 0.0, -100.0]; 2]
    );
    assert_eq!(app.core.model.nodes, nodes);
    assert_eq!(app.core.model.unassigned_beams[0].ends, supports);
    let path = std::env::temp_dir().join(format!("sepika-726-{}.ovika", std::process::id()));
    for value in [
        [[100.0, 200.0, -100.0], [-50.0, 40.0, 300.0]],
        [[0.0; 3]; 2],
    ] {
        app.core.model.unassigned_beams[0].end_offsets_mm = value;
        sepika_io::ovika::save_ovika(&path, &app.core.model, Default::default()).unwrap();
        let restored = sepika_io::ovika::load_ovika(&path).unwrap().model;
        assert_eq!(restored.unassigned_beams[0].end_offsets_mm, value);
        assert_eq!(restored.elements[2].end_offsets_mm, [[0.0, 0.0, 100.0]; 2]);
        assert_eq!(restored.unassigned_beams[0].ends, supports);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn beam_placement_transforms_analysis_and_preserves_connectivity_through_undo_and_reload() {
    let mut app = app();
    let model = &app.core.model;
    let before = sepika_solver::statics::linear::linear_static_once(model, LoadCaseId(0)).unwrap();
    let nodes = model.nodes.clone();
    let connectivity = model
        .elements
        .iter()
        .map(|e| (e.id, e.nodes.clone(), e.end_cond, e.rigid_zone))
        .collect::<Vec<_>>();
    assert!(app.apply_model_edit(Box::new(SetMemberEndOffsets {
        member: SupportMemberId::Primary(ElemId(2)),
        value: [[0.0, 0.0, 750.0]; 2]
    })));
    let model = &app.core.model;
    let after = sepika_solver::statics::linear::linear_static_once(model, LoadCaseId(0)).unwrap();
    assert_ne!(
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
    let path = std::env::temp_dir().join(format!("sepika-734-{}.ovika", std::process::id()));
    sepika_io::ovika::save_ovika(&path, model, Default::default()).unwrap();
    let restored = sepika_io::ovika::load_ovika(&path).unwrap().model;
    let reloaded =
        sepika_solver::statics::linear::linear_static_once(&restored, LoadCaseId(0)).unwrap();
    assert_eq!(
        rmp_serde::to_vec_named(&after).unwrap(),
        rmp_serde::to_vec_named(&reloaded).unwrap()
    );
    std::fs::remove_file(path).unwrap();
    app.undo_action();
    assert!(app.core.scoped.staleness.results_stale);
    let undone =
        sepika_solver::statics::linear::linear_static_once(&app.core.model, LoadCaseId(0)).unwrap();
    assert_eq!(
        rmp_serde::to_vec_named(&before).unwrap(),
        rmp_serde::to_vec_named(&undone).unwrap()
    );
}

#[test]
fn node_and_section_edits_invalidate_results_and_update_placement() {
    let mut app = app();
    let member = SupportMemberId::Primary(ElemId(2));
    assert!(app.apply_model_edit(Box::new(SetMemberEndOffsets {
        member,
        value: [[0.0, 0.0, 100.0]; 2]
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
        app.core.model.elements[2].end_offsets_mm,
        [[0.0, 0.0, 100.0]; 2]
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
    assert!(app.apply_model_edit(Box::new(SetMemberEndOffsets {
        member,
        value: [[0.0; 3]; 2]
    })));
    let revision = app.core.scoped.undo.revision();
    assert!(!app.apply_model_edit(Box::new(SetMemberEndOffsets {
        member,
        value: [[0.0, 0.0, f64::INFINITY]; 2]
    })));
    assert_eq!(app.core.scoped.undo.revision(), revision);
    assert!(app.core.model.beam_placement(member).is_ok());
}

#[test]
fn secondary_xyz_display_edits_preserve_calculation_evidence_and_freshness() {
    let mut app = app();
    app.core.scoped.staleness.mark_fresh();
    app.core.scoped.staleness.preparation_stale = false;
    app.core.scoped.staleness.design_stale = false;
    let modal = app.result_input(&result_validity::ResultInputKey::Modal);
    let baseline = app.core.model.calculation_model();
    let member = SupportMemberId::Secondary(sepika_core::ids::SecondaryMemberId(0));
    let before = app.core.model.beam_placement(member).unwrap().actual_ends;
    assert!(app.apply_model_edit(Box::new(SetMemberEndOffsets {
        member,
        value: [[100.0, 200.0, 300.0], [-200.0, 50.0, -100.0]]
    })));
    assert_ne!(
        app.core.model.beam_placement(member).unwrap().actual_ends,
        before
    );
    assert_eq!(
        app.core
            .model
            .calculation_beam_placement(member)
            .unwrap()
            .actual_ends,
        before
    );
    assert_eq!(
        app.result_input(&result_validity::ResultInputKey::Modal),
        modal
    );
    assert!(app
        .core
        .model
        .calculation_model()
        .eq_ignoring_dofmap(&baseline));
    assert!(!app.core.scoped.staleness.results_stale);
    assert!(!app.core.scoped.staleness.preparation_stale);
    assert!(!app.core.scoped.staleness.design_stale);
    app.undo_action();
    assert_eq!(
        app.core.model.beam_placement(member).unwrap().actual_ends,
        before
    );
    assert!(!app.core.scoped.staleness.results_stale);
    app.redo_action();
    assert_ne!(
        app.core.model.beam_placement(member).unwrap().actual_ends,
        before
    );
    assert!(!app.core.scoped.staleness.results_stale);
}

#[test]
fn column_and_post_xyz_edits_keep_distinct_freshness_and_round_trip_undo() {
    let mut app = app();
    let mut post = app.core.model.unassigned_beams.remove(0);
    post.kind = SecondaryMemberKind::Post;
    post.section = Some(SectionId(0));
    post.ends =
        SecondaryMemberEnds::Supported([0.25, 0.75].map(|position| SecondaryMemberAnchor {
            support: SupportMemberId::Primary(ElemId(0)),
            position,
        }));
    app.core.model.unassigned_posts.push(post);
    app.core.scoped.staleness.mark_fresh();
    app.core.scoped.staleness.preparation_stale = false;
    app.core.scoped.staleness.design_stale = false;
    let baseline = app.core.model.calculation_model();
    let offsets = [[100.0, 200.0, 300.0], [-400.0, 500.0, -600.0]];
    let secondary = SupportMemberId::Secondary(sepika_core::ids::SecondaryMemberId(0));
    let primary = SupportMemberId::Primary(ElemId(0));
    let nodes = app.core.model.nodes.clone();
    let ends = app.core.model.unassigned_posts[0].ends;
    assert!(app.apply_model_edit(Box::new(SetMemberEndOffsets {
        member: secondary,
        value: offsets
    })));
    assert!(!app.core.scoped.staleness.results_stale);
    assert!(!app.core.scoped.staleness.preparation_stale);
    assert!(!app.core.scoped.staleness.design_stale);
    assert!(app
        .core
        .model
        .calculation_model()
        .eq_ignoring_dofmap(&baseline));
    app.undo_action();
    assert_eq!(
        app.core.model.unassigned_posts[0].end_offsets_mm,
        [[0.0; 3]; 2]
    );
    app.redo_action();
    assert_eq!(app.core.model.unassigned_posts[0].end_offsets_mm, offsets);
    assert!(app.apply_model_edit(Box::new(SetMemberEndOffsets {
        member: primary,
        value: offsets
    })));
    assert!(app.core.scoped.staleness.results_stale);
    assert!(app.core.scoped.staleness.preparation_stale);
    assert!(app.core.scoped.staleness.design_stale);
    assert_eq!(app.core.model.nodes, nodes);
    assert_eq!(app.core.model.unassigned_posts[0].ends, ends);
    let path = std::env::temp_dir().join(format!("column-post-xyz-{}.ovika", std::process::id()));
    sepika_io::ovika::save_ovika(&path, &app.core.model, Default::default()).unwrap();
    let restored = sepika_io::ovika::load_ovika(&path).unwrap().model;
    assert_eq!(restored.elements[0].end_offsets_mm, offsets);
    assert_eq!(restored.unassigned_posts[0].end_offsets_mm, offsets);
    std::fs::remove_file(path).unwrap();
    app.undo_action();
    assert_eq!(app.core.model.elements[0].end_offsets_mm, [[0.0; 3]; 2]);
}

#[test]
fn xyz_brace_gui_edit_invalidates_results_and_round_trips_undo_save() {
    let mut app = app();
    app.core.model.elements[0].kind = sepika_core::model::ElementKind::Brace { tension_only: true };
    app.core.model.sections[0].frame_use = Some(sepika_core::model::FrameSectionUse::Brace);
    let nodes = app.core.model.nodes.clone();
    let offsets = [[100.0, 200.0, 300.0], [-400.0, 500.0, -600.0]];
    app.core.scoped.staleness.mark_fresh();
    app.core.scoped.staleness.preparation_stale = false;
    app.core.scoped.staleness.design_stale = false;
    assert!(app.apply_model_edit(Box::new(SetMemberEndOffsets {
        member: SupportMemberId::Primary(ElemId(0)),
        value: offsets
    })));
    assert!(app.core.scoped.staleness.results_stale);
    assert!(app.core.scoped.staleness.preparation_stale);
    assert!(app.core.scoped.staleness.design_stale);
    assert_eq!(app.core.model.nodes, nodes);
    let path = std::env::temp_dir().join(format!("brace-xyz-{}.ovika", std::process::id()));
    sepika_io::ovika::save_ovika(&path, &app.core.model, Default::default()).unwrap();
    let restored = sepika_io::ovika::load_ovika(&path).unwrap().model;
    assert_eq!(restored.elements[0].end_offsets_mm, offsets);
    assert_eq!(restored.nodes, nodes);
    let mut json = serde_json::to_value(&restored).unwrap();
    json["elements"][0]
        .as_object_mut()
        .unwrap()
        .remove("end_offsets_mm");
    assert!(serde_json::from_value::<sepika_core::model::Model>(json).is_err());
    std::fs::remove_file(path).unwrap();
    app.undo_action();
    assert_eq!(app.core.model.elements[0].end_offsets_mm, [[0.0; 3]; 2]);
    app.redo_action();
    assert_eq!(app.core.model.elements[0].end_offsets_mm, offsets);
}

#[test]
fn xyz_brace_study_copies_input_rejects_proposal_edits_and_keeps_main_identity() {
    use super::study::Location;
    let mut app = app();
    app.core.model.elements[0].kind = sepika_core::model::ElementKind::Brace {
        tension_only: false,
    };
    app.core.model.sections[0].frame_use = Some(sepika_core::model::FrameSectionUse::Brace);
    let offsets = [[100.0, 200.0, 300.0]; 2];
    assert!(app.apply_model_edit(Box::new(SetMemberEndOffsets {
        member: SupportMemberId::Primary(ElemId(0)),
        value: offsets
    })));
    let main = app.input_reference();
    let study = app
        .create_study("偏心入力を保持する検討".into(), vec![ElemId(2)])
        .unwrap();
    let proposal = app.create_comparison(study, "比較案".into()).unwrap();
    app.switch_study(Location::Proposal(study, proposal))
        .unwrap();
    assert_eq!(app.core.model.elements[0].end_offsets_mm, offsets);
    let reference = app.input_reference();
    let revision = app.core.scoped.undo.revision();
    assert!(!app.apply_model_edit(Box::new(SetMemberEndOffsets {
        member: SupportMemberId::Primary(ElemId(0)),
        value: [[0.0; 3]; 2]
    })));
    assert_eq!(app.input_reference(), reference);
    assert_eq!(app.core.scoped.undo.revision(), revision);
    assert_eq!(app.core.model.elements[0].end_offsets_mm, offsets);
    app.switch_study(Location::Main).unwrap();
    assert_eq!(app.input_reference(), main);
    assert!(app.apply_model_edit(Box::new(SetMemberEndOffsets {
        member: SupportMemberId::Primary(ElemId(0)),
        value: [[0.0; 3]; 2]
    })));
    assert_ne!(app.input_reference(), main);
    assert_eq!(
        app.studies.items[study].baseline.model.elements[0].end_offsets_mm,
        offsets
    );
}
