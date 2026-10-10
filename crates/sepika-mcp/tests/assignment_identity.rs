use sepika_core as core_fixture_api;
use sepika_mcp::{apply_edit, ServerState};
#[path = "../../sepika-core/tests/support/assignment_identity_model.rs"]
mod fixture;

#[test]
fn mcpは版荷重除去の確認前にはモデル履歴を更新せず確認後に一undoで戻せる() {
    for wall in [false, true] {
        let model = fixture::with_plate_metadata(wall);
        let dir =
            std::env::temp_dir().join(format!("sepika-identity-mcp-{}-{wall}", std::process::id()));
        let mut state = ServerState::with_fs_store(model, &dir).unwrap();
        state.model.assign_stb_node_ids().unwrap();
        let before = state.model.clone();
        let noop = apply_edit(
            &mut state,
            &serde_json::json!({"command":"DeleteSecondaryMember", "member":99}),
        )
        .unwrap();
        assert!(!noop.applied && !noop.undoable);
        assert_eq!(state.undo.revision(), 0);
        fixture::assert_inputs_eq(&state.model, &before);
        let mut body = serde_json::json!({ "command": "PlaceSecondaryMember", "parent": if wall { "wall" } else { "floor" }, "region": 0, "kind": if wall { "Post" } else { "Beam" }, "ends": fixture::divider(wall).ends });
        let reason = apply_edit(&mut state, &body).unwrap_err();
        assert!(reason.contains("未更新"));
        assert!(reason.contains("0.0025"));
        assert!(reason.contains("1 Undo"));
        fixture::assert_inputs_eq(&state.model, &before);
        assert_eq!(state.undo.revision(), 0);
        assert!(!state.undo.can_undo());
        body["confirm_plate_loss"] = true.into();
        let result = apply_edit(&mut state, &body).unwrap();
        assert!(result.applied && result.undoable);
        assert!(result.summary.contains("0.0025"));
        assert!(state.model.stb_strengths.members.is_empty());
        let after = state.model.clone();
        for (label, model) in [("before", &before), ("after", &after)] {
            let path = dir.join(format!("{label}.ovika"));
            sepika_io::ovika::save_ovika(&path, model, Default::default()).unwrap();
            let loaded = sepika_io::ovika::load_ovika(&path).unwrap().model;
            fixture::assert_inputs_eq(&loaded, model);
            assert_eq!(loaded.stb_strengths, model.stb_strengths);
            assert_eq!(
                loaded.seismic_weight_generation,
                model.seismic_weight_generation
            );
        }
        state.undo.undo(&mut state.model);
        fixture::assert_inputs_eq(&state.model, &before);
        state.undo.redo(&mut state.model);
        assert_eq!(state.model.next_secondary_member_id, 1);
        fixture::assert_inputs_eq(&state.model, &after);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
