#[path = "support/secondary_materialization_model.rs"]
mod fixture;
use sepika_core::{ids::*, model::*};

#[test]
fn materialized_beam_ignores_visual_eccentricity_in_either_end_order() {
    for reverse in [false, true] {
        for reverse_secondary in [false, true] {
            for (primary, secondary, expected) in [
                (-300.0, 0.0, false),
                (-300.0, -300.0, false),
                (0.0, -300.0, true),
                (0.0, 0.0, true),
            ] {
                let mut model = fixture::model();
                model.elements[0].end_offsets_mm = [[0.0, 0.0, primary]; 2];
                model.unassigned_beams[0].end_offsets_mm = [[0.0, 0.0, secondary]; 2];
                if reverse {
                    model.elements[0].nodes.reverse();
                }
                if reverse_secondary {
                    let SecondaryMemberEnds::Supported(ends) = &mut model.unassigned_beams[0].ends
                    else {
                        unreachable!()
                    };
                    ends.reverse();
                }
                assert_eq!(
                    model.secondary_member_materialized(&model.unassigned_beams[0]),
                    expected,
                    "primary={primary:?}, secondary={secondary:?}, reverse={reverse}"
                );
                assert_eq!(model.secondary_beam_axes().len(), usize::from(!expected));
            }
        }
    }
}

#[test]
fn materialization_requires_assigned_section_use_and_resolved_material() {
    let mut model = fixture::model();
    model.elements[0].end_offsets_mm = [[0.0, 0.0, 0.]; 2];
    let member = model.unassigned_beams[0].clone();
    assert!(model.secondary_member_materialized(&member));
    let mut other_section = model.sections[0].clone();
    other_section.id = SectionId(1);
    model.sections.push(other_section);
    model.elements[0].section = Some(SectionId(1));
    assert!(!model.secondary_member_materialized(&member));
    model.elements[0].section = Some(SectionId(0));
    model.sections[0].material = None;
    assert!(!model.secondary_member_materialized(&member));
    model.sections[0].material = Some(MaterialId(0));
    model.sections[0].frame_use = Some(FrameSectionUse::Column);
    assert!(!model.secondary_member_materialized(&member));
    model.sections[0].frame_use = Some(FrameSectionUse::Girder);
    model.unassigned_beams[0].section = None;
    model.elements[0].section = None;
    assert!(!model.secondary_member_materialized(&model.unassigned_beams[0]));
}

#[test]
fn materialization_matches_element_endpoints_without_selecting_first_nearby_node() {
    let mut model = fixture::model();
    model.elements[0].end_offsets_mm = [[0.0, 0.0, 0.]; 2];
    for coord in [[0., 5., 3000.], [6000., 5., 3000.]] {
        let mut node = model.nodes[0].clone();
        node.id = NodeId(model.nodes.len() as u32);
        node.coord = coord;
        model.nodes.push(node);
    }
    model.unassigned_beams[0].ends =
        SecondaryMemberEnds::Detached([[0., 0., 3000.], [6000., 0., 3000.]]);
    model.elements[0].nodes = vec![NodeId(4), NodeId(5)].into();
    assert!(!model.secondary_member_materialized(&model.unassigned_beams[0]));
    model.nodes[4].coord[1] = 0.0;
    model.nodes[5].coord[1] = 0.0;
    assert!(model.secondary_member_materialized(&model.unassigned_beams[0]));
    model.nodes[4].coord[1] = 11.;
    assert!(!model.secondary_member_materialized(&model.unassigned_beams[0]));
}

#[test]
fn posts_require_column_use_and_zero_analysis_offset() {
    let mut model = fixture::model();
    model.nodes[1].coord = [0., 0., 6000.];
    model.sections[0].frame_use = Some(FrameSectionUse::Column);
    model.unassigned_beams[0].kind = SecondaryMemberKind::Post;
    assert!(!model.secondary_member_materialized(&model.unassigned_beams[0]));
    model.elements[0].end_offsets_mm = [[0.0; 3]; 2];
    assert!(model.secondary_member_materialized(&model.unassigned_beams[0]));
    model.elements[0].kind = ElementKind::Fiber;
    assert!(!model.secondary_member_materialized(&model.unassigned_beams[0]));
}

#[test]
fn materialization_does_not_merge_distinct_resolved_member_materials() {
    let mut model = fixture::model();
    model.elements[0].end_offsets_mm = [[0.0, 0.0, 0.]; 2];
    model.sections[0].shape = None;
    model.materials[0].category = MaterialCategory::Concrete;
    model.materials[0].fc = Some(24.);
    let mut concrete = model.materials[0].clone();
    concrete.id = MaterialId(1);
    concrete.category = MaterialCategory::Concrete;
    concrete.fc = Some(30.);
    model.materials.push(concrete);
    let mut secondary_section = model.sections[0].clone();
    secondary_section.id = SectionId(1);
    secondary_section.material = Some(MaterialId(1));
    model.sections.push(secondary_section);
    model.unassigned_beams[0].section = Some(SectionId(1));
    assert_eq!(
        model
            .secondary_material(&model.unassigned_beams[0])
            .map(|m| m.id),
        Some(MaterialId(1))
    );
    assert_eq!(
        model.element_material(&model.elements[0]).map(|m| m.id),
        Some(MaterialId(0))
    );
    assert!(!model.secondary_member_materialized(&model.unassigned_beams[0]));
}

#[test]
fn post_materialization_never_turns_display_offsets_into_analysis_offsets() {
    let mut model = fixture::model();
    model.sections[0].frame_use = Some(FrameSectionUse::Column);
    let mut post = model.unassigned_beams.remove(0);
    post.kind = SecondaryMemberKind::Post;
    post.end_offsets_mm = [[100.0, 200.0, 300.0], [-400.0, 500.0, -600.0]];
    model.unassigned_posts.push(post.clone());
    let baseline = model.calculation_model();
    model.unassigned_posts[0].end_offsets_mm = [[0.0; 3]; 2];
    assert!(model.calculation_model().eq_ignoring_dofmap(&baseline));
    for reverse in [false, true] {
        if reverse {
            model.elements[0].nodes.reverse();
        }
        model.elements[0].end_offsets_mm = [[0.0; 3]; 2];
        assert!(model.secondary_member_materialized(&post));
        model.elements[0].end_offsets_mm = [[100.0, 0.0, 0.0]; 2];
        assert!(!model.secondary_member_materialized(&post));
    }
}
