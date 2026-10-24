use super::*;
use sepika_core as core_fixture_api;
use sepika_core::ids::SlabId;
use sepika_core::model::{Model, Slab, SlabShape};
use serde_json::json;
#[path = "../../sepika-core/tests/support/assignment_identity_model.rs"]
#[allow(dead_code)]
mod fixture;

#[tokio::test]
async fn actual_model_edit_body_rejects_overflow_without_mutation_and_normal_edit_undo_restores() {
    for assigned in [false, true] {
        let model = if assigned {
            fixture::with_plate_metadata(false)
        } else {
            let mut model = Model::default();
            model.slabs.push(Slab {
                id: SlabId(0),
                shape: SlabShape::Enclosed,
                plate: Default::default(),
                reference_story: None,
                vertical_eccentricity_mm: None,
                tip_loads: vec![],
            });
            model
        };
        let before = model.clone();
        let dir =
            std::env::temp_dir().join(format!("sepika-603-tool-{}-{assigned}", std::process::id()));
        let server = SepikaServer::new(ServerState::with_fs_store(model, &dir).unwrap());
        let tool_arguments = if assigned {
            json!({"body":{"command":"DeleteSlab","id":4294967296_u64,"confirm_plate_loss":true}})
        } else {
            json!({"body":{"command":"DeleteSlab","id":4294967296_u64}})
        };
        let args: EditArgs = serde_json::from_value(tool_arguments).unwrap();
        let error = server.model_edit(Parameters(args)).await.unwrap_err();
        assert!(
            error.message.contains("id") && error.message.contains("4294967295"),
            "{error:?}"
        );
        {
            let state = server.state.lock().await;
            assert!(state.model.eq_ignoring_dofmap(&before));
            assert_eq!(
                serde_json::to_value(&state.model).unwrap(),
                serde_json::to_value(&before).unwrap()
            );
            assert_eq!(state.undo.revision(), 0);
            assert!(!state.undo.can_undo() && !state.undo.can_redo());
        }
        let args: EditArgs = serde_json::from_value(
            json!({"body":{"command":"DeleteSlab","id":0,"confirm_plate_loss":true}}),
        )
        .unwrap();
        let result = server.model_edit(Parameters(args)).await.unwrap();
        assert!(!result.is_error.unwrap_or(false));
        let mut state = server.state.lock().await;
        assert!(state.model.slabs.is_empty());
        assert!(state.undo.can_undo());
        let ServerState { undo, model, .. } = &mut *state;
        undo.undo(model);
        assert!(model.eq_ignoring_dofmap(&before));
        assert!(undo.can_redo());
        drop(state);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
