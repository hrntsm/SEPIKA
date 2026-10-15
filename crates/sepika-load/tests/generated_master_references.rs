use sepika_core::{dof::Dof6Mask, ids::NodeId, model::*};
use sepika_load::story_gen::{
    generate_stories, generate_stories_multi, generate_stories_with_opts,
    generate_stories_with_synced_self_weight,
};
fn baseline() -> Model {
    Model {
        nodes: (0..3)
            .map(|id| Node {
                id: NodeId(id),
                coord: [0., 0., if id == 1 { 3000. } else { 0. }],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        generated_masters: vec![NodeId(2)],
        ..Default::default()
    }
}
#[test]
fn public_generation_rejects_absent_and_mismatched_owners_without_mutation() {
    for id in [3, u32::MAX, 1] {
        let mut model = baseline();
        model.generated_masters.push(NodeId(id));
        if id == 1 {
            model.nodes[1].id = NodeId(9);
        }
        let before = format!("{model:?}");
        for result in [
            generate_stories(&model, None),
            generate_stories_multi(&model, &[]),
            generate_stories_with_opts(&model, &[], false, MassMethod::default()),
            generate_stories_with_synced_self_weight(&model, &[], MassMethod::default()),
        ] {
            let error = result.unwrap_err();
            assert!(error.contains("generated_masters[1]"), "{error}");
            assert!(error.contains(&id.to_string()), "{error}");
        }
        assert_eq!(format!("{model:?}"), before);
    }
}
#[test]
fn existing_inactive_owner_is_reusable_without_mutating_input() {
    let model = baseline();
    let before = format!("{model:?}");
    let result = generate_stories(&model, None).unwrap();
    assert!(result.generated_masters.contains(&NodeId(2)));
    assert_eq!(format!("{model:?}"), before);
}
