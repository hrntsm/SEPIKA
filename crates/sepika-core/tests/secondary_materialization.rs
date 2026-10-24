#[path = "support/secondary_materialization_model.rs"]
mod fixture;
use sepika_core::{ids::*, model::*};

#[test]
fn materialized_beam_requires_matching_eccentricity_in_either_end_order() {
    for reverse in [false, true] {
        for reverse_secondary in [false, true] {
            for (primary, secondary, expected) in [
                (Some(-300.), Some(0.), false),
                (Some(-300.), Some(-300.), true),
                (None, Some(0.), false),
                (Some(0.), None, false),
                (None, None, true),
                (Some(0.), Some(0.), true),
            ] {
                let mut model = fixture::model();
                model.elements[0].vertical_eccentricity = primary;
                model.unassigned_beams[0].vertical_eccentricity = secondary;
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
    model.elements[0].vertical_eccentricity = Some(0.);
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
    model.elements[0].vertical_eccentricity = Some(0.);
    for coord in [[0., 5., 3000.], [6000., 5., 3000.]] {
        let mut node = model.nodes[0].clone();
        node.id = NodeId(model.nodes.len() as u32);
        node.coord = coord;
        model.nodes.push(node);
    }
    model.unassigned_beams[0].ends =
        SecondaryMemberEnds::Detached([[0., 0., 3000.], [6000., 0., 3000.]]);
    model.elements[0].nodes = vec![NodeId(4), NodeId(5)].into();
    assert!(model.secondary_member_materialized(&model.unassigned_beams[0]));
    model.nodes[4].coord[1] = 11.;
    assert!(!model.secondary_member_materialized(&model.unassigned_beams[0]));
}

#[test]
fn posts_keep_column_use_and_ignore_beam_eccentricity() {
    let mut model = fixture::model();
    model.nodes[1].coord = [0., 0., 6000.];
    model.sections[0].frame_use = Some(FrameSectionUse::Column);
    model.unassigned_beams[0].kind = SecondaryMemberKind::Post;
    assert!(model.secondary_member_materialized(&model.unassigned_beams[0]));
    model.elements[0].kind = ElementKind::Fiber;
    assert!(!model.secondary_member_materialized(&model.unassigned_beams[0]));
}

#[test]
fn materialization_does_not_merge_distinct_resolved_member_materials() {
    let mut model = fixture::model();
    model.elements[0].vertical_eccentricity = Some(0.);
    model.sections[0].shape = None;
    model.materials[0].category = MaterialCategory::Concrete;
    model.materials[0].fc = Some(24.);
    let mut concrete = model.materials[0].clone();
    concrete.id = MaterialId(1);
    concrete.category = MaterialCategory::Concrete;
    concrete.fc = Some(30.);
    model.materials.push(concrete);
    model.stb_strengths.materials.push(StbGradeMaterial {
        grade: "Fc30".into(),
        material: MaterialId(1),
    });
    model.stb_strengths.members.push(StbMemberStrength {
        target: StrengthTarget::Secondary(SecondaryMemberId(0)),
        node: NodeId(0),
        node_order: vec![NodeId(0), NodeId(1)],
        concrete: Some("Fc30".into()),
    });
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
