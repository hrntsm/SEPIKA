use super::*;
use sepika_core::ids::*;
#[path = "../../sepika-app/tests/support/nonnodal_quantity_model.rs"]
mod fixture;

fn value(result: CallToolResult) -> serde_json::Value {
    serde_json::from_str(&result.content[0].raw.as_text().unwrap().text).unwrap()
}

async fn quantity(server: &SepikaServer, group: &str) -> serde_json::Value {
    value(
        server
            .quantity_takeoff(Parameters(QuantityTakeoffArgs {
                group_by: Some(group.into()),
            }))
            .await
            .unwrap(),
    )
}

fn store(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("sepika557-mcp-{}-{name}", std::process::id()))
}

#[tokio::test]
async fn nonnodal557_mcp_handler_groups_keep_secondary_identity_and_unavailable() {
    let model = fixture::placed();
    model.validate().unwrap();
    let before = serde_json::to_vec(&model).unwrap();
    let dir = store("groups");
    let server = SepikaServer::new(ServerState::with_fs_store(model, &dir).unwrap());
    for group in ["category", "story", "steel", "detail", "items"] {
        let q = quantity(&server, group).await;
        assert_ne!(q["status"], "unavailable", "{q}");
        assert!(
            (q["totals"]["concrete_m3"].as_f64().unwrap() - 0.54).abs() < 1e-12,
            "{q}"
        );
        assert!(
            (q["totals"]["formwork_m2"].as_f64().unwrap() - 4.5).abs() < 1e-12,
            "{q}"
        );
        let rows = q["rows"].as_array().unwrap();
        match group {
            "detail" | "items" => {
                let rows: Vec<_> = rows
                    .iter()
                    .filter(|r| r["secondary_member"] == 557)
                    .collect();
                assert_eq!(rows.len(), 1);
                assert!(rows[0]["elem"].is_null() && rows[0]["slab"].is_null());
                assert_eq!(rows[0]["label"], "非節点小梁");
                assert_eq!(rows[0]["story"], "1F");
                assert!(q["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|r| !r["elem"].is_null())
                    .all(|r| r["secondary_member"].is_null()));
            }
            "story" => assert!(rows.iter().any(|r| r["story"] == "1F")),
            "steel" => {
                assert_eq!(rows.len(), 1);
                assert!((rows[0]["length_m"].as_f64().unwrap() - 12.0).abs() < 1e-12);
                assert!((rows[0]["weight_t"].as_f64().unwrap() - 0.942).abs() < 1e-12);
            }
            "category" => assert!(rows.iter().any(|r| r["category"] == "小梁")),
            _ => unreachable!(),
        }
    }
    {
        let mut st = server.state.lock().await;
        assert_eq!(serde_json::to_vec(&st.model).unwrap(), before);
        assert_eq!(st.undo.revision(), 0);
        assert!(!st.undo.can_undo());
        st.model.sections[0].material = Some(MaterialId(999));
    }
    for group in ["category", "story", "steel", "detail"] {
        let q = quantity(&server, group).await;
        assert_eq!(q["status"], "unavailable");
        assert!(q["totals"].is_null());
        assert!(
            q["reason"]
                .as_str()
                .unwrap()
                .contains("SecondaryMemberId(557)"),
            "{q}"
        );
        assert!(q.get("rows").is_none());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn nonnodal557_mcp_supported_edit_handler_undo_redo_and_save_roundtrip() {
    let dir = store("edit");
    let server =
        SepikaServer::new(ServerState::with_fs_store(fixture::edit_model(), &dir).unwrap());
    let result = value(
        server
            .model_edit(Parameters(EditArgs {
                body: serde_json::json!({
                    "command":"PlaceSecondaryMember", "parent":"unassigned", "kind":"Beam",
                    "ends":fixture::ends(0.5), "section":0, "name":"編集小梁"
                }),
            }))
            .await
            .unwrap(),
    );
    assert_eq!(result["applied"], true, "{result}");
    let (id, placed) = {
        let mut st = server.state.lock().await;
        let id = st.model.beams().next().unwrap().id;
        st.model = fixture::with_stb_strengths(st.model.clone(), id);
        assert_eq!(
            st.model
                .secondary_material(st.model.secondary_member(id).unwrap())
                .unwrap()
                .fc,
            Some(30.0)
        );
        fixture::assert_quantity(&st.model, id, 0.54, 4.5);
        (id, st.model.clone())
    };
    let q = quantity(&server, "detail").await;
    assert!(
        q["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["secondary_member"] == id.0 && r["concrete_m3"] == 0.54),
        "{q}"
    );
    let result = value(
        server
            .model_edit(Parameters(EditArgs {
                body: serde_json::json!({
                    "command":"SetSecondaryMemberEnds", "member":id.0, "ends":fixture::moved_ends()
                }),
            }))
            .await
            .unwrap(),
    );
    assert_eq!(result["applied"], true, "{result}");
    let moved = {
        let mut st = server.state.lock().await;
        fixture::assert_quantity(&st.model, id, 0.675, 5.625);
        let moved = st.model.clone();
        {
            let ServerState { undo, model, .. } = &mut *st;
            undo.undo(model);
        }
        assert!(st.model.eq_ignoring_dofmap(&placed));
        assert_eq!(st.model.stb_strengths, placed.stb_strengths);
        fixture::assert_quantity(&st.model, id, 0.54, 4.5);
        moved
    };
    assert!(
        (quantity(&server, "category").await["totals"]["concrete_m3"]
            .as_f64()
            .unwrap()
            - 0.54)
            .abs()
            < 1e-12
    );
    {
        let mut st = server.state.lock().await;
        {
            let ServerState { undo, model, .. } = &mut *st;
            undo.redo(model);
        }
        assert!(st.model.eq_ignoring_dofmap(&moved));
        fixture::assert_quantity(&st.model, id, 0.675, 5.625);
        let revision = st.undo.revision();
        let path = dir.join("edited.ovika");
        sepika_io::ovika::save_ovika(&path, &st.model, Default::default()).unwrap();
        let loaded = sepika_io::ovika::load_ovika(&path).unwrap().model;
        assert!(loaded.eq_ignoring_dofmap(&moved));
        assert_eq!(loaded.stb_strengths, placed.stb_strengths);
        assert_eq!(
            loaded
                .secondary_material(loaded.secondary_member(id).unwrap())
                .unwrap()
                .fc,
            Some(30.0)
        );
        assert_eq!(
            loaded.secondary_member(id).unwrap().ends,
            fixture::moved_ends()
        );
        fixture::assert_quantity(&loaded, id, 0.675, 5.625);
        assert_eq!(st.undo.revision(), revision);
    }
    let q = quantity(&server, "detail").await;
    let row = q["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["secondary_member"] == id.0)
        .unwrap();
    assert!((row["concrete_m3"].as_f64().unwrap() - 0.675).abs() < 1e-12);
    assert_eq!(row["story"], "1F");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn nonnodal557_mcp_handler_retains_unknown_story_and_fallback_label() {
    let mut model = fixture::placed();
    for node in &mut model.nodes {
        node.story = None;
    }
    model.floor_regions[0].secondary_beams[0].name.clear();
    let dir = store("unknown-story");
    let server = SepikaServer::new(ServerState::with_fs_store(model, &dir).unwrap());
    for group in ["category", "story", "steel", "detail"] {
        let q = quantity(&server, group).await;
        assert_ne!(q["status"], "unavailable");
        assert!((q["totals"]["concrete_m3"].as_f64().unwrap() - 0.54).abs() < 1e-12);
        assert!(q["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n == "SecondaryMemberId(557): 階帰属未確定（数量は階 - に計上）"));
        if group == "story" {
            assert!(q["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["story"] == "-"
                    && (r["concrete_m3"].as_f64().unwrap() - 0.54).abs() < 1e-12));
        }
        if group == "detail" {
            let row = q["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["secondary_member"] == 557)
                .unwrap();
            assert_eq!(row["label"], "B300x600");
            assert_eq!(row["story"], "-");
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}
