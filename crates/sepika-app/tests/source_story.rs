use sepika_core::ids::{NodeId, StoryId};
use sepika_core::model::{MassMethod, Model, SourceStoryKind};
use sepika_edit::UndoStack;

fn model() -> Model {
    sepika_io::stbridge::import_stbridge(r#"<ST_BRIDGE version="2.0.2"><StbModel>
      <StbNodes>
        <StbNode id="10" guid="11111111-1111-1111-1111-111111111111" X="0" Y="0" Z="-3000"/>
        <StbNode id="40" X="0" Y="0" Z="0"/>
        <StbNode id="70" X="0" Y="0" Z="3100"/>
        <StbNode id="80" X="0" Y="0" Z="6000"/>
        <StbNode id="90" X="0" Y="0" Z="9000"/>
      </StbNodes>
      <StbStories>
        <StbStory id="50" guid="22222222-2222-2222-2222-222222222222" name="PH" height="9000" kind="PENTHOUSE"><StbNodeIdList><StbNodeId id="90"/></StbNodeIdList></StbStory>
        <StbStory id="80" name="B1" height="-3000" kind="BASEMENT"><StbNodeIdList><StbNodeId id="10"/></StbNodeIdList></StbStory>
        <StbStory id="20" name="1F" height="0" kind="GENERAL"><StbNodeIdList><StbNodeId id="40"/></StbNodeIdList></StbStory>
        <StbStory id="5" name="段差" height="3000" kind="DEPENDENCE" id_dependence="20" strength_concrete="FC27"><StbNodeIdList><StbNodeId id="70"/></StbNodeIdList></StbStory>
        <StbStory id="2" name="RF" height="6000" kind="ROOF"><StbNodeIdList><StbNodeId id="80"/></StbNodeIdList></StbStory>
      </StbStories>
    </StbModel></ST_BRIDGE>"#).unwrap()
}

fn prepare(model: &mut Model, undo: &mut UndoStack) {
    let g = sepika_load::story_gen::generate_stories(model, None).unwrap();
    assert!(undo.run(
        model,
        Box::new(sepika_edit::ApplyStories {
            damper_mass_generation: g.damper_mass_generation,
            stories: g.stories,
            node_story: g.node_story,
            constraints: g.constraints,
            rep_nodes: g.rep_nodes,
            generated_masters: g.generated_masters,
            mass_method: MassMethod::default(),
        })
    ));
}

#[test]
fn source_stories_prepare_twice_move_export_and_fc_reference_are_invariant() {
    let mut m = model();
    let source = m.source_stories.clone();
    let identities = m.stb_node_ids.clone();
    let mut undo = UndoStack::new();
    assert_eq!(
        m.resolve_source_concrete_fc(NodeId(2), None, None, Some(21.0)),
        Ok(Some(27.0))
    );
    prepare(&mut m, &mut undo);
    assert_eq!(m.nodes[2].story, Some(StoryId(3)));
    assert_eq!(m.source_stories, source);
    assert!(!m.source_story_assignment_diagnostics().is_empty());
    prepare(&mut m, &mut undo);
    assert_eq!(m.source_stories, source);
    assert!(undo.run(
        &mut m,
        Box::new(sepika_edit::SetNodeCoord {
            node: NodeId(2),
            coord: [0.0, 0.0, 6100.0]
        })
    ));
    prepare(&mut m, &mut undo);
    assert_eq!(m.source_stories, source);
    assert_eq!(m.stb_node_ids, identities);
    assert_eq!(
        m.resolve_source_concrete_fc(NodeId(2), None, None, Some(21.0)),
        Ok(Some(27.0))
    );
    let export = sepika_io::stbridge::export_stbridge(&m).unwrap();
    let restored = sepika_io::stbridge::import_stbridge(&export).unwrap();
    assert_eq!(restored.source_stories, source);
    assert_eq!(restored.stb_node_ids, identities);
    assert_eq!(export, sepika_io::stbridge::export_stbridge(&m).unwrap());
    assert!(export.contains("kind=\"DEPENDENCE\" id_dependence=\"20\" strength_concrete=\"FC27\""));
}

#[test]
fn source_membership_delete_undo_redo_and_new_ids_survive_internal_renumbering() {
    let mut m = model();
    let original = m.clone();
    let mut undo = UndoStack::new();
    assert!(undo.run(&mut m, Box::new(sepika_edit::DeleteNode { id: NodeId(1) })));
    assert!(m
        .source_stories
        .iter()
        .find(|s| s.id == 20)
        .unwrap()
        .node_ids
        .is_empty());
    assert!(m.validate().is_ok());
    assert_eq!(
        m.stb_node_ids
            .iter()
            .find(|n| n.node == NodeId(1))
            .unwrap()
            .id,
        70
    );
    undo.undo(&mut m);
    assert!(m.eq_ignoring_dofmap(&original));
    undo.redo(&mut m);
    assert!(m.validate().is_ok());
    undo.undo(&mut m);
    assert!(undo.run(
        &mut m,
        Box::new(sepika_edit::SetSourceStoryNodes {
            source_story: 5,
            nodes: vec![NodeId(3)]
        })
    ));
    assert_eq!(
        m.source_stories
            .iter()
            .find(|s| s.id == 5)
            .unwrap()
            .node_ids[0]
            .id,
        80
    );
    let changed = m.source_stories.clone();
    undo.undo(&mut m);
    assert_eq!(m.source_stories, original.source_stories);
    undo.redo(&mut m);
    assert_eq!(m.source_stories, changed);
    assert!(undo.run(
        &mut m,
        Box::new(sepika_edit::AddNode {
            coord: [1.0, 2.0, 3.0],
            restraint: sepika_core::dof::Dof6Mask::FREE
        })
    ));
    let new_external = m.stb_node_ids.last().unwrap().id;
    assert!(new_external > 90);
    undo.undo(&mut m);
    undo.redo(&mut m);
    assert_eq!(m.stb_node_ids.last().unwrap().id, new_external);
    assert!(undo.run(&mut m, Box::new(sepika_edit::DeleteNode { id: NodeId(1) })));
    assert_eq!(m.stb_node_ids.last().unwrap().id, new_external);
    assert!(m.validate().is_ok());
}

#[test]
fn source_conflicts_diagnose_independent_story_processing_and_resolve_upper_fc() {
    let mut m = model();
    let mut duplicate = m.source_stories.iter().find(|s| s.id == 5).unwrap().clone();
    duplicate.id = 100;
    duplicate.height = 4000.0;
    duplicate.kind = SourceStoryKind::General;
    duplicate.id_dependence = None;
    duplicate.strength_concrete = Some("FC24".into());
    m.source_stories.push(duplicate);
    assert_eq!(
        m.resolve_source_concrete_fc(NodeId(2), Some(36.0), Some(30.0), Some(21.0)),
        Ok(Some(36.0))
    );
    assert_eq!(
        m.resolve_source_concrete_fc(NodeId(2), None, Some(30.0), Some(21.0)),
        Ok(Some(30.0))
    );
    assert!(m
        .resolve_source_concrete_fc(NodeId(2), None, None, Some(21.0))
        .unwrap_err()
        .contains("原階Fcが競合"));
    let error = sepika_load::story_gen::generate_stories(&m, None).unwrap_err();
    assert!(error.contains("階依存処理の所属が未確定"));
    assert!(!error.contains("原階Fc"));
    assert!(sepika_solver::statics::analysis::precheck::model_issues(&m)
        .iter()
        .any(|issue| issue.message.contains("階依存処理の所属が未確定")));
    m.source_stories.last_mut().unwrap().strength_concrete = None;
    assert!(m
        .resolve_source_concrete_fc(NodeId(2), None, None, Some(21.0))
        .is_err());
    m.source_stories.last_mut().unwrap().strength_concrete = Some("unknown".into());
    assert!(m
        .resolve_source_concrete_fc(NodeId(2), None, None, Some(21.0))
        .unwrap_err()
        .contains("解決できません"));
    assert_eq!(
        m.resolve_source_concrete_fc(NodeId(2), Some(36.0), None, Some(21.0)),
        Ok(Some(36.0))
    );
}

#[test]
fn source_duplicate_height_dependency_missing_self_cycle_and_unknown_nodes_are_rejected() {
    for case in 0..5 {
        let mut m = model();
        let index = m.source_stories.iter().position(|s| s.id == 5).unwrap();
        let expected = match case {
            0 => {
                m.source_stories[index].height = 0.0;
                "同一height"
            }
            1 => {
                m.source_stories[index].id_dependence = None;
                "id_dependence がありません"
            }
            2 => {
                m.source_stories[index].id_dependence = Some(5);
                "自己参照"
            }
            3 => {
                let base = m.source_stories.iter_mut().find(|s| s.id == 20).unwrap();
                base.id_dependence = Some(5);
                "循環"
            }
            _ => {
                m.source_stories[index]
                    .node_ids
                    .push(sepika_core::model::SourceStoryNode {
                        id: 999,
                        node: None,
                    });
                "未知の節点ID"
            }
        };
        let error = sepika_load::story_gen::generate_stories(&m, None).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
    let mut m = model();
    m.source_stories
        .iter_mut()
        .find(|s| s.id == 5)
        .unwrap()
        .id_dependence = Some(999);
    assert!(m
        .source_story_diagnostics()
        .iter()
        .any(|s| s.contains("存在しません")));
}

#[test]
fn source_required_attributes_and_lower_priority_fc_are_checked_when_selected() {
    let m = model();
    assert_eq!(
        m.resolve_source_concrete_fc(NodeId(2), None, None, Some(-1.0)),
        Ok(Some(27.0))
    );
    assert!(m
        .resolve_source_concrete_fc(NodeId(1), None, None, Some(-1.0))
        .is_err());
    for attributes in [
        "id=\"1\" height=\"0\" kind=\"GENERAL\"",
        "id=\"1\" name=\"F\" height=\"0\"",
    ] {
        let xml = format!("<ST_BRIDGE version=\"2.0.2\"><StbModel><StbStories><StbStory {attributes}/></StbStories></StbModel></ST_BRIDGE>");
        assert!(sepika_io::stbridge::import_stbridge(&xml).is_err());
    }
}

#[test]
fn source_new_story_ids_and_guid_survive_height_order_changes_and_undo() {
    let mut m = model();
    let source = m.source_stories.clone();
    let mut undo = UndoStack::new();
    assert!(undo.run(
        &mut m,
        Box::new(sepika_edit::AddStory {
            name: "新規".into(),
            elevation: 4500.0
        })
    ));
    let new_id = m.source_stories.last().unwrap().id;
    assert!(new_id > 80);
    let index = m.stories.iter().position(|s| s.name == "新規").unwrap();
    assert!(undo.run(
        &mut m,
        Box::new(sepika_edit::SetStoryLevel {
            story: StoryId(index as u32),
            name: "新規2".into(),
            elevation: 8000.0
        })
    ));
    assert_eq!(
        m.source_stories
            .iter()
            .find(|s| s.id == new_id)
            .unwrap()
            .height,
        8000.0
    );
    assert_eq!(
        m.source_stories.iter().find(|s| s.id == 50).unwrap().guid,
        source.iter().find(|s| s.id == 50).unwrap().guid
    );
    undo.undo(&mut m);
    undo.undo(&mut m);
    assert_eq!(m.source_stories, source);
    undo.redo(&mut m);
    assert_eq!(m.source_stories.last().unwrap().id, new_id);
}

#[test]
fn public_source_fixture_roundtrip_preserves_original_attributes_and_identity() {
    let fixture = include_str!("fixtures/public_source_stories_497.stb");
    let original = sepika_io::stbridge::import_stbridge(fixture).unwrap();
    let export = sepika_io::stbridge::export_stbridge(&original).unwrap();
    let restored = sepika_io::stbridge::import_stbridge(&export).unwrap();
    assert_eq!(original.source_stories, restored.source_stories);
    assert_eq!(original.stb_node_ids, restored.stb_node_ids);
    assert_eq!(original.source_stories.len(), 6);
    assert_eq!(original.stb_node_ids.len(), 126);
}

#[test]
fn source_native_save_and_calculation_snapshot_preserve_the_explicit_table() {
    let mut m = model();
    let snapshot = m.clone();
    let mut undo = UndoStack::new();
    let initial_revision = undo.revision();
    assert!(undo.run(
        &mut m,
        Box::new(sepika_edit::SetSourceStoryNodes {
            source_story: 5,
            nodes: vec![NodeId(1)]
        })
    ));
    assert_eq!(undo.revision(), initial_revision + 1);
    assert_eq!(snapshot.source_stories, model().source_stories);
    assert_ne!(snapshot.source_stories, m.source_stories);
    let path = std::env::temp_dir().join(format!("source-story-497-{}.ovika", std::process::id()));
    sepika_io::ovika::save_ovika(&path, &m, Default::default()).unwrap();
    let restored = sepika_io::ovika::load_ovika(&path).unwrap();
    assert_eq!(restored.model.source_stories, m.source_stories);
    assert_eq!(restored.model.stb_node_ids, m.stb_node_ids);
    std::fs::remove_file(path).unwrap();
    undo.undo(&mut m);
    assert_eq!(undo.revision(), initial_revision + 2);
    assert!(m.eq_ignoring_dofmap(&snapshot));
    undo.redo(&mut m);
    assert_eq!(undo.revision(), initial_revision + 3);
}

#[test]
fn native_first_delete_keeps_existing_export_ids_and_undo_restores_unassigned_table() {
    let mut m = model();
    m.source_stories.clear();
    m.source_stories_initialized = false;
    m.stories.clear();
    m.stb_node_ids.clear();
    for node in &mut m.nodes {
        node.story = None;
    }
    let original = m.clone();
    let mut undo = UndoStack::new();
    let before = sepika_io::stbridge::export_stbridge(&m).unwrap();
    assert!(undo.run(&mut m, Box::new(sepika_edit::DeleteNode { id: NodeId(0) })));
    let exported = sepika_io::stbridge::export_stbridge(&m).unwrap();
    let restored = sepika_io::stbridge::import_stbridge(&exported).unwrap();
    assert_eq!(
        restored
            .stb_node_ids
            .iter()
            .map(|n| n.id)
            .collect::<Vec<_>>(),
        vec![2, 3, 4, 5]
    );
    undo.undo(&mut m);
    assert!(m.eq_ignoring_dofmap(&original));
    assert_eq!(sepika_io::stbridge::export_stbridge(&m).unwrap(), before);
    undo.redo(&mut m);
    assert_eq!(sepika_io::stbridge::export_stbridge(&m).unwrap(), exported);
}

#[test]
fn native_story_delete_preserves_source_ids_and_restores_uninitialized_tables() {
    let mut m = model();
    m.source_stories.clear();
    m.source_stories_initialized = false;
    m.stb_node_ids.clear();
    let original = m.clone();
    let before = sepika_io::stbridge::export_stbridge(&m).unwrap();
    let before_stories = sepika_io::stbridge::import_stbridge(&before)
        .unwrap()
        .source_stories;
    let removed_name = m.stories[1].name.clone();
    let mut undo = UndoStack::new();
    for id in [StoryId(0), StoryId(u32::MAX)] {
        assert!(!undo.run(&mut m, Box::new(sepika_edit::DeleteStory { story: id })));
        assert!(m.eq_ignoring_dofmap(&original));
        assert_eq!(undo.revision(), 0);
    }
    assert!(undo.run(
        &mut m,
        Box::new(sepika_edit::DeleteStory { story: StoryId(1) })
    ));
    let exported = sepika_io::stbridge::export_stbridge(&m).unwrap();
    let restored = sepika_io::stbridge::import_stbridge(&exported).unwrap();
    assert_eq!(
        restored.source_stories,
        before_stories
            .into_iter()
            .filter(|s| s.name != removed_name)
            .collect::<Vec<_>>()
    );
    assert!(m.validate().is_ok());
    undo.undo(&mut m);
    assert!(m.eq_ignoring_dofmap(&original));
    assert!(m.source_stories.is_empty());
    assert!(m.stb_node_ids.is_empty());
    assert_eq!(sepika_io::stbridge::export_stbridge(&m).unwrap(), before);
    undo.redo(&mut m);
    assert_eq!(sepika_io::stbridge::export_stbridge(&m).unwrap(), exported);
    assert!(m.validate().is_ok());

    let mut referenced = model();
    let original = referenced.clone();
    let target = referenced
        .stories
        .iter()
        .find(|s| s.elevation == 0.0)
        .unwrap()
        .id;
    let mut undo = UndoStack::new();
    assert!(!undo.run(
        &mut referenced,
        Box::new(sepika_edit::DeleteStory { story: target })
    ));
    assert!(undo.last_error().unwrap().contains("従属階から参照"));
    assert!(referenced.eq_ignoring_dofmap(&original));
    assert_eq!(undo.revision(), 0);
}

#[test]
fn empty_import_and_last_source_delete_stay_empty_through_edit_prepare_save_and_undo() {
    let envelope = |stories: &str| {
        format!(
            r#"<ST_BRIDGE version="2.0.2"><StbModel><StbNodes><StbNode id="1" X="0" Y="0" Z="0"/><StbNode id="2" X="0" Y="0" Z="3000"/></StbNodes><StbStories>{stories}</StbStories></StbModel></ST_BRIDGE>"#
        )
    };
    for initially_empty in [false, true] {
        let mut m = sepika_io::stbridge::import_stbridge(&envelope(if initially_empty { "" } else {
            r#"<StbStory id="50" name="upper" height="3000" kind="ROOF"><StbNodeIdList><StbNodeId id="2"/></StbNodeIdList></StbStory>"#
        })).unwrap();
        assert!(m.source_stories_initialized);
        let mut undo = UndoStack::new();
        prepare(&mut m, &mut undo);
        let before_delete = m.clone();
        if !initially_empty {
            assert!(undo.run(
                &mut m,
                Box::new(sepika_edit::DeleteStory { story: StoryId(1) })
            ));
            let deleted = m.clone();
            undo.undo(&mut m);
            assert!(m.eq_ignoring_dofmap(&before_delete));
            undo.redo(&mut m);
            assert!(m.eq_ignoring_dofmap(&deleted));
        }
        assert!(m.source_stories.is_empty());
        let assert_empty_export = |m: &Model| {
            let xml = sepika_io::stbridge::export_stbridge(m).unwrap();
            assert!(!xml.contains("<StbStory"));
            assert_eq!(xml, sepika_io::stbridge::export_stbridge(m).unwrap());
            let restored = sepika_io::stbridge::import_stbridge(&xml).unwrap();
            assert!(restored.source_stories.is_empty());
            assert!(restored.source_stories_initialized);
        };
        assert_empty_export(&m);
        let before_edit = m.clone();
        assert!(undo.run(
            &mut m,
            Box::new(sepika_edit::SetStoryLevel {
                story: StoryId(0),
                name: "明示基部名".into(),
                elevation: 0.0,
            })
        ));
        assert!(m.source_stories.is_empty());
        undo.undo(&mut m);
        assert!(m.eq_ignoring_dofmap(&before_edit));
        undo.redo(&mut m);
        assert!(undo.run(
            &mut m,
            Box::new(sepika_edit::SetStoryLevelKind {
                story: StoryId(0),
                level_kind: sepika_core::model::StoryLevelKind::Penthouse { k: 0.7 },
            })
        ));
        assert_empty_export(&m);
        let before_reject = m.clone();
        let revision = undo.revision();
        assert!(!undo.run(
            &mut m,
            Box::new(sepika_edit::DeleteStory { story: StoryId(0) })
        ));
        assert!(!undo.run(
            &mut m,
            Box::new(sepika_edit::SetStoryLevelKind {
                story: StoryId(u32::MAX),
                level_kind: Default::default(),
            })
        ));
        assert!(m.eq_ignoring_dofmap(&before_reject));
        assert_eq!(undo.revision(), revision);
        prepare(&mut m, &mut undo);
        prepare(&mut m, &mut undo);
        assert!(m.source_stories.is_empty());
        assert_empty_export(&m);
        let path = std::env::temp_dir().join(format!(
            "497-empty-{}-{initially_empty}.ovika",
            std::process::id()
        ));
        sepika_io::ovika::save_ovika(&path, &m, Default::default()).unwrap();
        let restored = sepika_io::ovika::load_ovika(&path).unwrap().model;
        assert!(restored.eq_ignoring_dofmap(&m));
        assert_empty_export(&restored);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn main_native_model_without_source_fields_keeps_messagepack_export_fallback() {
    let mut m = model();
    m.source_stories.clear();
    m.source_stories_initialized = false;
    m.stb_node_ids.clear();
    let mut bytes = rmp_serde::to_vec(&m).unwrap();
    assert_eq!(bytes.pop(), Some(0xc0));
    assert_eq!(bytes.pop(), Some(0xc2));
    assert_eq!(bytes.pop(), Some(0x90));
    assert_eq!(bytes.pop(), Some(0x90));
    assert_eq!(bytes[0], 0xdc);
    let fields = u16::from_be_bytes([bytes[1], bytes[2]]) - 4;
    bytes[1..3].copy_from_slice(&fields.to_be_bytes());
    let restored: Model = rmp_serde::from_slice(&bytes).unwrap();
    assert!(!restored.source_stories_initialized);
    assert!(restored.eq_ignoring_dofmap(&m));
    assert_eq!(
        sepika_io::stbridge::export_stbridge(&restored).unwrap(),
        sepika_io::stbridge::export_stbridge(&m).unwrap()
    );
    assert!(!sepika_io::stbridge::import_stbridge(
        &sepika_io::stbridge::export_stbridge(&restored).unwrap()
    )
    .unwrap()
    .source_stories
    .is_empty());
}

#[test]
fn native_first_story_edit_preserves_both_membership_forms_and_conflict_diagnostics() {
    for representation in 0..4 {
        let mut m = model();
        m.source_stories.clear();
        m.source_stories_initialized = false;
        m.stb_node_ids.clear();
        match representation {
            0 => {
                for story in &mut m.stories {
                    story.node_ids.clear();
                }
            }
            1 => {
                for node in &mut m.nodes {
                    node.story = None;
                }
            }
            2 => {}
            3 => m.nodes[0].story = Some(StoryId(1)),
            _ => unreachable!(),
        }
        m.generated_masters.push(NodeId(4));
        assert!(m.validate().is_ok());
        let path = std::env::temp_dir().join(format!(
            "native-membership-497-{}-{representation}.ovika",
            std::process::id()
        ));
        sepika_io::ovika::save_ovika(&path, &m, Default::default()).unwrap();
        let saved = sepika_io::ovika::load_ovika(&path).unwrap().model;
        std::fs::remove_file(path).unwrap();
        assert!(saved.eq_ignoring_dofmap(&m));
        m = saved;
        let original = m.clone();
        let before = sepika_io::stbridge::export_stbridge(&m).unwrap();
        let mut expected = sepika_io::stbridge::import_stbridge(&before)
            .unwrap()
            .source_stories;
        let memberships: Vec<Vec<u32>> = expected
            .iter()
            .map(|story| story.node_ids.iter().map(|n| n.id).collect())
            .collect();
        assert_eq!(
            memberships,
            vec![
                vec![1],
                if representation == 3 {
                    vec![1, 2]
                } else {
                    vec![2]
                },
                vec![3],
                vec![4],
                vec![],
            ]
        );
        let mut undo = UndoStack::new();
        let base_elevation = m.stories[0].elevation;
        for story in [StoryId(u32::MAX), StoryId(1)] {
            assert!(!undo.run(
                &mut m,
                Box::new(sepika_edit::SetStoryLevel {
                    story,
                    name: "rejected".into(),
                    elevation: base_elevation,
                })
            ));
            assert!(m.eq_ignoring_dofmap(&original));
        }
        let elevation = m.stories[1].elevation;
        assert!(undo.run(
            &mut m,
            Box::new(sepika_edit::SetStoryLevel {
                story: StoryId(1),
                name: "renamed".into(),
                elevation,
            })
        ));
        expected[1].name = "renamed".into();
        let after = sepika_io::stbridge::export_stbridge(&m).unwrap();
        let restored = sepika_io::stbridge::import_stbridge(&after).unwrap();
        assert_eq!(restored.source_stories, expected);
        assert!(!m.stb_node_ids.iter().any(|n| n.node == NodeId(4)));
        assert_eq!(
            m.source_story_diagnostics()
                .iter()
                .any(|d| d.contains("多重所属")),
            representation == 3
        );
        undo.undo(&mut m);
        assert!(m.eq_ignoring_dofmap(&original));
        assert_eq!(sepika_io::stbridge::export_stbridge(&m).unwrap(), before);
        undo.redo(&mut m);
        assert_eq!(sepika_io::stbridge::export_stbridge(&m).unwrap(), after);
        assert_eq!(sepika_io::stbridge::export_stbridge(&m).unwrap(), after);
    }
}
