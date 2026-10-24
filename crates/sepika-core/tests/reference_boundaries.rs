use sepika_core::{
    dof::{Dof, Dof6Mask},
    ids::*,
    model::*,
};
fn baseline() -> Model {
    Model {
        nodes: (0..3)
        .map(|i| Node {
            id: NodeId(i),
            coord: [i as f64 * 1000., 0., 0.],
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        })
        .collect(),
        stories: vec![serde_json::from_value(serde_json::json!({"id":0,"name":"base","elevation":0.,"node_ids":[],"seismic_weight":null})).unwrap()],
        load_cases: default_load_cases(),
        ..Default::default()
    }
}
fn record(m: &mut Model) -> &mut SeismicWeightGeneration {
    m.seismic_weight_generation
        .get_or_insert(SeismicWeightGeneration {
            input_key: vec![],
            output_key: vec![],
            calculated_weights: vec![],
            automatic_node_stories: Vec::new(),
            automatic_diaphragms: vec![],
            automatic_master_restraints: vec![],
        })
}
fn attach(m: &mut Model, target: &str, n: NodeId) {
    match target {
        "node.story" => m.nodes[0].story = Some(StoryId(n.0)),
        "record.calculated_story" => record(m).calculated_weights.push((StoryId(n.0), 1.)),
        "record.diaphragm.story" => {
            record(m)
                .automatic_diaphragms
                .push(Constraint::rigid_diaphragm(
                    StoryId(n.0),
                    NodeId(0),
                    vec![NodeId(1)],
                ))
        }
        "story.nodes" => m.stories[0].node_ids = vec![n],
        "diaphragm.master" => {
            m.constraints
                .push(Constraint::rigid_diaphragm(StoryId(0), n, vec![NodeId(1)]))
        }
        "diaphragm.slave" => {
            m.constraints
                .push(Constraint::rigid_diaphragm(StoryId(0), NodeId(0), vec![n]))
        }
        "diaphragm.story" => m.constraints.push(Constraint::rigid_diaphragm(
            StoryId(n.0),
            NodeId(0),
            vec![NodeId(1)],
        )),
        "link.master" => m.constraints.push(Constraint::RigidLink {
            master: n,
            slaves: vec![NodeId(1)],
            dofs: Dof6Mask::FIXED,
        }),
        "link.slave" => m.constraints.push(Constraint::RigidLink {
            master: NodeId(0),
            slaves: vec![n],
            dofs: Dof6Mask::FIXED,
        }),
        "mpc.master" => m.constraints.push(Constraint::Mpc {
            master: n,
            terms: vec![(NodeId(1), Dof::Ux, 1.)],
        }),
        "mpc.term" => m.constraints.push(Constraint::Mpc {
            master: NodeId(0),
            terms: vec![(n, Dof::Ux, 1.)],
        }),
        "nodal.manual" => m.load_cases[0]
            .nodal
            .push(NodalLoad::manual(n, [1., 0., 0., 0., 0., 0.])),
        "nodal.auto" => m.load_cases[0]
            .nodal
            .push(NodalLoad::auto(n, [1., 0., 0., 0., 0., 0.])),
        "generated_masters" => m.generated_masters = vec![n],
        "record.diaphragm.master" => record(m)
            .automatic_diaphragms
            .push(Constraint::rigid_diaphragm(StoryId(0), n, vec![NodeId(1)])),
        "record.diaphragm.slave" => record(m)
            .automatic_diaphragms
            .push(Constraint::rigid_diaphragm(StoryId(0), NodeId(0), vec![n])),
        "record.master_restraint" => record(m)
            .automatic_master_restraints
            .push((n, Dof6Mask::FIXED)),
        "record.link.master" => record(m).automatic_diaphragms.push(Constraint::RigidLink {
            master: n,
            slaves: vec![NodeId(1)],
            dofs: Dof6Mask::FIXED,
        }),
        "record.link.slave" => record(m).automatic_diaphragms.push(Constraint::RigidLink {
            master: NodeId(0),
            slaves: vec![n],
            dofs: Dof6Mask::FIXED,
        }),
        "record.mpc.master" => record(m).automatic_diaphragms.push(Constraint::Mpc {
            master: n,
            terms: vec![(NodeId(1), Dof::Ux, 1.)],
        }),
        "record.mpc.term" => record(m).automatic_diaphragms.push(Constraint::Mpc {
            master: NodeId(0),
            terms: vec![(n, Dof::Ux, 1.)],
        }),
        _ => unreachable!(),
    }
}
const TARGETS: &[&str] = &[
    "node.story",
    "record.calculated_story",
    "record.diaphragm.story",
    "story.nodes",
    "diaphragm.master",
    "diaphragm.slave",
    "diaphragm.story",
    "link.master",
    "link.slave",
    "mpc.master",
    "mpc.term",
    "nodal.manual",
    "nodal.auto",
    "generated_masters",
    "record.diaphragm.master",
    "record.diaphragm.slave",
    "record.master_restraint",
    "record.link.master",
    "record.link.slave",
    "record.mpc.master",
    "record.mpc.term",
];
fn is_story(target: &str) -> bool {
    matches!(
        target,
        "node.story" | "record.calculated_story" | "record.diaphragm.story" | "diaphragm.story"
    )
}

#[test]
fn typed_references_require_existing_targets_without_mutation() {
    for &target in TARGETS {
        let unset = baseline();
        assert!(unset.validate_reference_ids().is_ok(), "{target}");
        let mut valid = baseline();
        attach(&mut valid, target, NodeId(0));
        assert!(valid.validate_reference_ids().is_ok(), "{target}");
        for missing in [3, u32::MAX] {
            let mut model = baseline();
            attach(&mut model, target, NodeId(missing));
            let before = serde_json::to_value(&model).unwrap();
            let error = model.validate_reference_ids().unwrap_err().to_string();
            assert!(error.contains(&missing.to_string()), "{target}: {error}");
            assert!(
                error.contains(if is_story(target) { "Story" } else { "Node" }),
                "{target}: {error}"
            );
            assert!(model.validate().is_err(), "{target}");
            assert_eq!(serde_json::to_value(&model).unwrap(), before, "{target}");
        }
    }
}
#[test]
fn stale_generation_values_and_inactive_owners_are_valid() {
    let mut model = baseline();
    model.generated_masters = vec![NodeId(2)];
    model.nodes[2].restraint = Dof6Mask::FREE;
    let record = record(&mut model);
    record.input_key = vec![255];
    record.output_key = vec![254];
    record.calculated_weights = vec![(StoryId(0), 999.)];
    record.automatic_diaphragms = vec![Constraint::rigid_diaphragm(
        StoryId(0),
        NodeId(2),
        vec![NodeId(1)],
    )];
    record.automatic_master_restraints = vec![(NodeId(2), Dof6Mask::FIXED)];
    assert!(model.validate_reference_ids().is_ok());
    assert!(!model.is_automatic_seismic_master_restraint(NodeId(2)));
    model
        .constraints
        .push(Constraint::rigid_diaphragm(StoryId(0), NodeId(0), vec![]));
    assert!(model.validate_reference_ids().is_ok());
    assert!(Model::default().validate_reference_ids().is_ok());
}
