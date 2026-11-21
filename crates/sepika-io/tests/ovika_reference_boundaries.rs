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
    let manifest = serde_json::json!({"schema_version":1,"units":"internal: N-mm-s","created_by":"independent probe681","entries":[{"name":"model.msgpack","sha256":format!("{:x}",Sha256::digest(&model))}]});
    let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let opts = zip::write::FileOptions::<()>::default();
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("model.msgpack", model),
    ] {
        z.start_file(name, opts).unwrap();
        z.write_all(&bytes).unwrap();
    }
    z.finish().unwrap();
}

#[test]
fn ovika_rejects_steel_design_attr_without_a_member() {
    let mut model = baseline();
    model.steel_design_attrs = vec![serde_json::from_value(serde_json::json!({
        "elem": 0,
        "lk_y_direct": 1000.0
    }))
    .unwrap()];
    let path = std::env::temp_dir().join(format!("sepika1751-{}.ovika", std::process::id()));
    independent_file(&path, &model);
    let original = std::fs::read(&path).unwrap();

    let load = load_ovika(&path).err().unwrap();
    let save = save_ovika(&path, &model, OvikaExtras::default()).unwrap_err();
    for error in [load, save] {
        assert!(
            error.to_string().contains("鉄骨部材の検定属性 -> 部材 0"),
            "{error}"
        );
    }
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn ovika_rejects_joint_and_haunch_refs_to_missing_members() {
    let path =
        std::env::temp_dir().join(format!("sepika1762-invalid-{}.ovika", std::process::id()));
    let mut model = baseline();
    for attr in [
        MemberDetailAttr {
            joints: vec![MemberJoint {
                distance: 800.0,
                kind: Default::default(),
            }],
            ..MemberDetailAttr::new(ElemId(2))
        },
        MemberDetailAttr {
            haunch_j: Some(Haunch {
                length: 700.0,
                depth_increase: 100.0,
                width_increase: 25.0,
            }),
            ..MemberDetailAttr::new(ElemId(2))
        },
    ] {
        model.member_detail_attrs = vec![attr];
        independent_file(&path, &model);
        let original = std::fs::read(&path).unwrap();

        let load = load_ovika(&path).err().unwrap();
        let save = save_ovika(&path, &model, OvikaExtras::default()).unwrap_err();
        for error in [load, save] {
            assert!(
                error
                    .to_string()
                    .contains("部材の付帯情報（継手・ハンチ） -> 部材 2"),
                "{error}"
            );
        }
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn ovika_rejects_wall_attributes_without_a_wall_or_shell() {
    let mut model = baseline();
    model.elements.push(ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
        section: None,
        local_axis: LocalAxis {
            ref_vector: [0.0, 0.0, 1.0],
        },
        end_cond: [EndCondition::Fixed; 2],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        end_offsets_mm: [[0.0; 3]; 2],
        plastic_zone: None,
        spring: None,
    });
    let path =
        std::env::temp_dir().join(format!("sepika1764-invalid-{}.ovika", std::process::id()));
    for (id, reason) in [
        (0, "壁要素・シェルのいずれでもありません"),
        (7, "dangling reference"),
    ] {
        model.wall_attrs = vec![WallAttr {
            elem: ElemId(id),
            opening_area: 100_000.0,
            opening_weight: 20.0,
            openings: vec![],
            slit: WallSlit {
                column_face: [true, false],
                ..Default::default()
            },
            finish_intensity: 0.001,
        }];
        independent_file(&path, &model);
        let original = std::fs::read(&path).unwrap();
        let load = load_ovika(&path).err().unwrap();
        let save = save_ovika(&path, &model, OvikaExtras::default()).unwrap_err();
        for error in [load, save] {
            let message = error.to_string();
            assert!(message.contains("壁属性（開口・耐震スリット・仕上げ）"));
            assert!(message.contains(&format!("部材 {id}")), "{message}");
            assert!(message.contains(reason), "{message}");
        }
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn ovika_rejects_each_hysteresis_attribute_without_a_member() {
    use HysteresisModel::{Auto, Retrograde, Takeda};

    let path =
        std::env::temp_dir().join(format!("sepika1755-invalid-{}.ovika", std::process::id()));
    for (rule, rule_th, rc_beam_reference) in [
        (Takeda, None, None),
        (Auto, Some(Retrograde), None),
        (Auto, None, Some(RcBeamReference::AntisymmetricHalfMember)),
    ] {
        let mut model = baseline();
        model.member_hysteresis_attrs.push(MemberHysteresisAttr {
            elem: ElemId(0),
            rule,
            rule_th,
            rc_beam_reference,
        });
        independent_file(&path, &model);
        let original = std::fs::read(&path).unwrap();
        let load = load_ovika(&path).err().unwrap();
        let save = save_ovika(&path, &model, OvikaExtras::default()).unwrap_err();
        for error in [load, save] {
            assert!(
                error.to_string().contains("部材の履歴則指定 -> 部材 0"),
                "{error}"
            );
        }
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn ovika_rejects_a_damper_attr_without_a_member() {
    let mut model = baseline();
    model.set_damper_props(ElemId(7), Some(Default::default()));
    let path =
        std::env::temp_dir().join(format!("sepika1763-invalid-{}.ovika", std::process::id()));
    independent_file(&path, &model);
    let original = std::fs::read(&path).unwrap();
    let load = load_ovika(&path).err().unwrap();
    let save = save_ovika(&path, &model, OvikaExtras::default()).unwrap_err();
    for error in [load, save] {
        assert!(
            error.to_string().contains("制振ダンパー属性 -> 部材 7"),
            "{error}"
        );
    }
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn ovika_preserves_hysteresis_selection_and_rc_beam_reference_for_both_analyses() {
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, RebarSize, SectionShape};
    use sepika_element::factory::{ensure_nonlinear_input_for_kind, resolve_member_hysteresis};

    let mut model = baseline();
    model.nodes[1].coord[0] = 6000.0;
    model.materials.push(Material {
        id: MaterialId(0),
        name: "Fc24".into(),
        category: MaterialCategory::Concrete,
        young: 20_000.0,
        poisson: 0.2,
        density: 2.4e-9,
        shear: None,
        fc: Some(24.0),
        fy: None,
        concrete_class: Default::default(),
        strength_factor: None,
    });
    model.materials.push(Material {
        id: MaterialId(1),
        name: "SD345".into(),
        category: MaterialCategory::Rebar,
        young: 200_000.0,
        poisson: 0.3,
        fc: None,
        fy: Some(345.0),
        ..model.materials[0].clone()
    });
    let mut section = SectionShape::RcBeamRect {
        b: 300.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: RebarSize::DiameterMm(28.0),
            top: vec![3],
            bottom: vec![3],
            cover: 35.0,
            stirrup: BeamStirrup {
                dia: RebarSize::DiameterMm(10.0),
                pitch: 100.0,
                legs: 2,
            },
        },
    }
    .to_section(SectionId(0), "G1".into());
    section.frame_use = Some(FrameSectionUse::Girder);
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = Some(MaterialId(1));
    model.sections.push(section);
    let id = ElemId(0);
    model.elements.push(ElementData {
        id,
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 0.0, 1.0],
        },
        end_cond: [EndCondition::Fixed; 2],
        force_regime: ForceRegime::UniaxialBendingShear,
        rigid_zone: Default::default(),
        end_offsets_mm: [[0.0; 3]; 2],
        plastic_zone: None,
        spring: None,
    });
    let reference = Some(RcBeamReference::AntisymmetricHalfMember);
    model.member_hysteresis_attrs = vec![
        MemberHysteresisAttr {
            elem: id,
            rule: HysteresisModel::Retrograde,
            rule_th: Some(HysteresisModel::MaxPointOriented),
            rc_beam_reference: reference,
        },
        MemberHysteresisAttr {
            elem: id,
            rule: HysteresisModel::Takeda,
            rule_th: Some(HysteresisModel::OriginOriented),
            rc_beam_reference: None,
        },
    ];
    let path = std::env::temp_dir().join(format!("sepika1755-valid-{}.ovika", std::process::id()));
    save_ovika(&path, &model, OvikaExtras::default()).unwrap();
    let saved = load_ovika(&path).unwrap().model;
    std::fs::remove_file(path).unwrap();

    assert_eq!(saved.member_hysteresis_attrs, model.member_hysteresis_attrs);
    assert_eq!(saved.member_rc_beam_reference(id), reference);
    model = saved.clone();
    model.set_member_rc_beam_reference(id, None);
    for (kind, expected) in [
        (AnalysisKind::Incremental, HysteresisModel::Retrograde),
        (AnalysisKind::TimeHistory, HysteresisModel::MaxPointOriented),
    ] {
        assert_eq!(
            resolve_member_hysteresis(&saved.elements[0], &saved, kind),
            expected
        );
        ensure_nonlinear_input_for_kind(&saved, kind).unwrap();
        let error = ensure_nonlinear_input_for_kind(&model, kind).unwrap_err();
        assert!(error.contains("基準分布が未指定"), "{error}");
    }
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
