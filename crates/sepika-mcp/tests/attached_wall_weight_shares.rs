use sepika_core::{dof::Dof6Mask, ids::NodeId, model::*};
use sepika_io::ovika::{load_ovika, save_ovika};
use sepika_mcp::{apply_edit, ServerState};
use serde_json::{json, Value};

fn model() -> Model {
    Model {
        nodes: [0., 3000.]
            .into_iter()
            .enumerate()
            .map(|(id, x)| Node {
                id: NodeId(id as u32),
                coord: [x, 0., 0.],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        ..Default::default()
    }
}

fn add() -> Value {
    json!({
        "command": "AddAttachedWallPlate",
        "anchor": {"Line": {"nodes": [0, 1], "span": [0., 1.], "transfer": "Anchor"}},
        "extent": [900., 900.],
        "opening_area": 120000.,
        "opening_weight": 500.
    })
}

#[test]
fn attached_wall_rejects_add_time_shares_and_saves_shares_set_after_addition() {
    let dir = std::env::temp_dir().join(format!("sepika-wall-shares-{}", uuid::Uuid::new_v4()));
    let mut state = ServerState::with_fs_store(model(), &dir).unwrap();
    let before = serde_json::to_value(&state.model).unwrap();
    let revision = state.undo.revision();
    for shares in [json!([0.1, 0.2, 0.3, 0.4]), json!([]), Value::Null] {
        let mut command = add();
        command["self_weight_shares"] = shares;
        for body in [command.clone(), json!({"body": command})] {
            let error = apply_edit(&mut state, &body).unwrap_err();
            assert!(error.contains("AddAttachedWallPlate"), "{error}");
            assert!(error.contains("self_weight_shares"), "{error}");
            assert!(error.contains("対応していません"), "{error}");
            assert_eq!(serde_json::to_value(&state.model).unwrap(), before);
            assert_eq!(state.undo.revision(), revision);
            assert!(!state.undo.can_undo());
            assert!(!state.undo.can_redo());
        }
    }

    let result = apply_edit(&mut state, &add()).unwrap();
    assert!(result.applied && result.undoable);
    assert_eq!(state.model.wall_plates.len(), 1);
    let path = dir.join("wall.ovika");
    save_ovika(&path, &state.model, Default::default()).unwrap();
    let added = load_ovika(&path).unwrap().model;
    assert!(added.wall_plates[0].self_weight_shares.is_empty());
    assert_eq!(added.wall_plates[0].opening_area, 120000.);
    assert_eq!(added.wall_plates[0].opening_weight, 500.);

    let result = apply_edit(
        &mut state,
        &json!({"body": {
            "command": "SetWallPlateAttrs", "id": 0,
            "self_weight_shares": [0.1, 0.2, 0.3, 0.4],
            "opening_area": 120000., "opening_weight": 500.
        }}),
    )
    .unwrap();
    assert!(result.applied && result.undoable);
    save_ovika(&path, &state.model, Default::default()).unwrap();
    let updated = load_ovika(&path).unwrap().model;
    assert_eq!(
        updated.wall_plates[0].self_weight_shares,
        [0.1, 0.2, 0.3, 0.4]
    );
    assert_eq!(updated.wall_plates[0].opening_area, 120000.);
    assert_eq!(updated.wall_plates[0].opening_weight, 500.);
    let error = sepika_job::auto_loads::compute_gravity_auto_load_cases(&updated)
        .err()
        .expect("取り付く壁版の任意辺負担率は重力荷重生成で拒否される");
    assert_eq!(error.kind(), "invalid_input");
    assert!(
        error
            .to_string()
            .contains("取付き壁に梁三方式・任意辺率は適用できません"),
        "{error}"
    );
    state.undo.undo(&mut state.model);
    assert!(state.model.wall_plates[0].self_weight_shares.is_empty());
    state.undo.redo(&mut state.model);
    assert_eq!(
        state.model.wall_plates[0].self_weight_shares,
        [0.1, 0.2, 0.3, 0.4]
    );
    drop(state);
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn mcp_tool_reports_unsupported_add_time_shares_as_invalid_params() {
    use rmcp::handler::server::wrapper::Parameters;
    use sepika_mcp::server::{EditArgs, QueryArgs, SepikaServer};

    let dir =
        std::env::temp_dir().join(format!("sepika-wall-shares-tool-{}", uuid::Uuid::new_v4()));
    let server = SepikaServer::new(ServerState::with_fs_store(model(), &dir).unwrap());
    let mut command = add();
    command["self_weight_shares"] = json!([0.1, 0.2, 0.3, 0.4]);
    let error = server
        .model_edit(Parameters(EditArgs { body: command }))
        .await
        .unwrap_err();
    assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    assert!(error.message.contains("self_weight_shares"));
    assert!(error.message.contains("対応していません"));
    let result = server
        .model_query(Parameters(QueryArgs {
            kind: "wall_plate".into(),
            filter: None,
        }))
        .await
        .unwrap();
    let queried: Value =
        serde_json::from_str(&result.content[0].raw.as_text().unwrap().text).unwrap();
    assert_eq!(queried, json!({"items": []}));
    let result = server
        .model_edit(Parameters(EditArgs { body: add() }))
        .await
        .unwrap();
    let response: Value =
        serde_json::from_str(&result.content[0].raw.as_text().unwrap().text).unwrap();
    assert_eq!(response["applied"], true);
    assert_eq!(response["undoable"], true);
    assert_eq!(response["op_id"], "op-1");
    drop(server);
    std::fs::remove_dir_all(dir).unwrap();
}
