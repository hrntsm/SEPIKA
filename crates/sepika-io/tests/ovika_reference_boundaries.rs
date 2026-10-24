use sepika_core::{
    dof::{Dof, Dof6Mask},
    ids::*,
    model::*,
};
use sepika_io::ovika::{load_ovika, save_ovika, OvikaExtras};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path};
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
fn independent_file(path: &Path, m: &Model) {
    let model = rmp_serde::to_vec_named(m).unwrap();
    let settings = b"{}";
    let manifest = serde_json::json!({"schema_version":1,"units":"internal: N-mm-s","created_by":"independent probe681","entries":[{"name":"model.msgpack","sha256":format!("{:x}",Sha256::digest(&model))},{"name":"settings.json","sha256":format!("{:x}",Sha256::digest(settings))}]});
    let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let opts = zip::write::FileOptions::<()>::default();
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("model.msgpack", model),
        ("settings.json", settings.to_vec()),
    ] {
        z.start_file(name, opts).unwrap();
        z.write_all(&bytes).unwrap();
    }
    z.finish().unwrap();
}

#[test]
fn save_and_independent_load_reject_missing_targets_and_preserve_input() {
    let dir = std::env::temp_dir().join(format!("sepika681-reference-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("references.ovika");
    for &target in TARGETS {
        for id in [0, 3, u32::MAX] {
            let mut model = baseline();
            attach(&mut model, target, NodeId(id));
            let before = rmp_serde::to_vec_named(&model).unwrap();
            std::fs::write(&path, b"existing project").unwrap();
            let saved = save_ovika(&path, &model, OvikaExtras::default());
            assert_eq!(saved.is_ok(), id == 0, "{target}: {saved:?}");
            if id != 0 {
                assert_eq!(std::fs::read(&path).unwrap(), b"existing project");
            }
            independent_file(&path, &model);
            let bytes = std::fs::read(&path).unwrap();
            let loaded = load_ovika(&path);
            assert_eq!(loaded.is_ok(), id == 0, "{target}");
            match loaded {
                Ok(loaded) => assert_eq!(rmp_serde::to_vec_named(&loaded.model).unwrap(), before),
                Err(error) => {
                    let error = error.to_string();
                    assert!(error.contains(&id.to_string()), "{target}: {error}");
                    assert!(error.contains("dangling"), "{target}: {error}");
                }
            }
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert_eq!(rmp_serde::to_vec_named(&model).unwrap(), before);
        }
    }
    let model = Model::default();
    save_ovika(&path, &model, OvikaExtras::default()).unwrap();
    assert_eq!(
        rmp_serde::to_vec_named(&load_ovika(&path).unwrap().model).unwrap(),
        rmp_serde::to_vec_named(&model).unwrap()
    );
    let mut stale = baseline();
    stale.generated_masters = vec![NodeId(2)];
    let record = record(&mut stale);
    record.input_key = vec![255];
    record.output_key = vec![];
    record.calculated_weights = vec![(StoryId(0), 999.)];
    record.automatic_diaphragms = vec![Constraint::rigid_diaphragm(
        StoryId(0),
        NodeId(2),
        vec![NodeId(1)],
    )];
    record.automatic_master_restraints = vec![(NodeId(2), Dof6Mask::FIXED)];
    save_ovika(&path, &stale, OvikaExtras::default()).unwrap();
    assert_eq!(
        rmp_serde::to_vec_named(&load_ovika(&path).unwrap().model).unwrap(),
        rmp_serde::to_vec_named(&stale).unwrap()
    );
    std::fs::remove_dir_all(dir).unwrap();
}
