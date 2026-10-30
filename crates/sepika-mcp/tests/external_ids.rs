use sepika_core as core_fixture_api;
use sepika_core::ids::*;
use sepika_core::model::{Model, Slab, SlabShape};
use sepika_mcp::{apply_edit, parse_edit_command, ServerState};
use serde_json::{json, Value};
#[path = "../../sepika-core/tests/support/assignment_identity_model.rs"]
#[allow(dead_code)]
mod fixture;

fn invalid_ids() -> Vec<Value> {
    vec![
        json!(4294967296_u64),
        json!(u64::MAX),
        json!(-1),
        json!(0.5),
        json!(1.0),
        json!("0"),
        Value::Null,
        json!(true),
        json!([]),
        json!({}),
    ]
}

#[test]
fn handwritten_id_parsers_enforce_integer_type_and_u32_bounds() {
    let ends = serde_json::to_value(fixture::divider(false).ends).unwrap();
    let mut commands = vec![
        (json!({"command":"DeleteSlab"}), "id"),
        (json!({"command":"SetSlabSection"}), "id"),
        (json!({"command":"SetSlabUsage"}), "id"),
        (json!({"command":"SetSlabOneWay"}), "id"),
        (json!({"command":"SetAttachedExtent","extent":[1,1]}), "id"),
        (json!({"command":"DeleteWallPlate"}), "id"),
        (json!({"command":"SetWallPlateSection"}), "id"),
        (json!({"command":"SetWallPlateAttrs"}), "id"),
        (json!({"command":"SetAttachedWallPlateExtent"}), "id"),
        (json!({"command":"SetFloorRegionName","name":"floor"}), "id"),
        (
            json!({"command":"SetFloorRegionSecondaryBeams","secondary_beams":[]}),
            "floor_region",
        ),
        (
            json!({"command":"SetWallRegionPosts","posts":[]}),
            "wall_region",
        ),
        (
            json!({"command":"PlaceSecondaryMember","parent":"floor","kind":"Beam","ends":ends}),
            "region",
        ),
        (
            json!({"command":"PlaceSecondaryMember","parent":"wall","kind":"Post","ends":ends}),
            "region",
        ),
        (json!({"command":"DeleteSecondaryMember"}), "member"),
        (
            json!({"command":"SetSecondaryMemberEnds","ends":ends}),
            "member",
        ),
        (
            json!({"command":"SetSecondaryMemberEndSupport","end_support":["Supported","Free"]}),
            "member",
        ),
        (json!({"command":"SetPostGravityEndShares"}), "member"),
    ];
    for command in [
        "AssignSlabToFloorPlateRegion",
        "SetFloorPlateRegionNoPlate",
        "UnsetFloorPlateRegion",
        "AssignWallPlateToRegion",
        "SetWallPlateRegionNoPlate",
        "UnsetWallPlateRegion",
    ] {
        commands.push((json!({"command":command}), "region"));
    }
    for (mut command, field) in commands {
        assert!(
            parse_edit_command(&command).is_err(),
            "missing {field}: {command}"
        );
        for value in invalid_ids() {
            command[field] = value;
            let error = parse_edit_command(&command)
                .err()
                .expect("invalid ID must fail");
            assert!(error.contains(field), "{command}: {error}");
            assert!(error.contains("4294967295"), "{command}: {error}");
            assert!(parse_edit_command(&json!({"body":command})).is_err());
        }
        for valid in [0_u32, u32::MAX] {
            command[field] = json!(valid);
            assert!(parse_edit_command(&command).is_ok(), "{command}");
        }
    }
}

#[test]
fn optional_sections_distinguish_absence_and_null_from_invalid_specification() {
    for command in [
        "SetSlabSection",
        "SetWallPlateSection",
        "AssignSlabToFloorPlateRegion",
        "AssignWallPlateToRegion",
    ] {
        let mut payload = json!({"command":command,"id":0,"region":0});
        assert!(parse_edit_command(&payload).is_ok());
        payload["section"] = Value::Null;
        assert!(parse_edit_command(&payload).is_ok());
        for value in invalid_ids().into_iter().filter(|v| !v.is_null()) {
            payload["section"] = value.clone();
            let error = parse_edit_command(&payload).err().unwrap();
            assert!(error.contains("section"), "{error}");
            if command == "AssignSlabToFloorPlateRegion" {
                payload.as_object_mut().unwrap().remove("section");
                payload["plate"] = json!({"section":value,"loads":[],"usage":null,"method":"TriTrapezoid","one_way":null});
                assert!(parse_edit_command(&payload).is_err());
                payload.as_object_mut().unwrap().remove("plate");
            }
        }
        for valid in [0_u32, u32::MAX] {
            payload["section"] = json!(valid);
            assert!(parse_edit_command(&payload).is_ok());
        }
    }
    for (command, primary) in [
        ("SetFloorRegionSecondaryBeams", "floor_region"),
        ("SetWallRegionPosts", "wall_region"),
    ] {
        let body = json!({"command":command, primary:4294967296_u64,"id":0,"secondary_beams":[],"posts":[]});
        assert!(parse_edit_command(&body).err().unwrap().contains(primary));
    }
}

fn slab_model(assigned: bool) -> Model {
    if assigned {
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
    }
}

#[test]
fn rejected_edit_preserves_slab_zero_assignments_and_undo_redo() {
    for assigned in [false, true] {
        let dir =
            std::env::temp_dir().join(format!("sepika-603-edit-{}-{assigned}", std::process::id()));
        let mut state = ServerState::with_fs_store(slab_model(assigned), &dir).unwrap();
        let before = state.model.clone();
        for value in invalid_ids() {
            let body = json!({"command":"DeleteSlab","id":value,"confirm_plate_loss":true});
            let reason = apply_edit(&mut state, &body).unwrap_err();
            assert!(
                reason.contains("id") && reason.contains("4294967295"),
                "{reason}"
            );
            assert!(state.model.eq_ignoring_dofmap(&before));
            assert_eq!(
                serde_json::to_value(&state.model).unwrap(),
                serde_json::to_value(&before).unwrap()
            );
            assert_eq!(state.undo.revision(), 0);
            assert!(!state.undo.can_undo() && !state.undo.can_redo());
        }
        assert!(
            apply_edit(
                &mut state,
                &json!({"command":"DeleteSlab","id":0,"confirm_plate_loss":true})
            )
            .unwrap()
            .applied
        );
        assert!(state.model.slabs.is_empty());
        state.undo.undo(&mut state.model);
        assert!(state.model.eq_ignoring_dofmap(&before));
        assert_eq!(
            serde_json::to_value(&state.model).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
        let revision = state.undo.revision();
        assert!(state.undo.can_redo());
        assert!(apply_edit(
            &mut state,
            &json!({"command":"DeleteSlab","id":4294967296_u64,"confirm_plate_loss":true})
        )
        .is_err());
        assert!(state.model.eq_ignoring_dofmap(&before));
        assert_eq!(
            serde_json::to_value(&state.model).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
        assert_eq!(state.undo.revision(), revision);
        assert!(state.undo.can_redo());
        state.undo.redo(&mut state.model);
        assert!(state.model.slabs.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn nested_serde_ids_reject_invalid_values_in_secondary_members_anchors_and_plate_sections() {
    let mut member = serde_json::to_value(fixture::divider(false)).unwrap();
    for field in ["id", "section"] {
        for value in invalid_ids()
            .into_iter()
            .filter(|v| field != "section" || !v.is_null())
        {
            member[field] = value;
            let body = json!({"command":"AddUnassignedBeam","beam":member});
            assert!(parse_edit_command(&body).err().unwrap().contains("beam"));
        }
        member[field] = json!(0);
        assert!(parse_edit_command(&json!({"command":"AddUnassignedBeam","beam":member})).is_ok());
        member[field] = json!(u32::MAX);
        assert!(parse_edit_command(&json!({"command":"AddUnassignedBeam","beam":member})).is_ok());
        member[field] = json!(0);
    }
    for command in [
        "AddAttachedSlab",
        "AddAttachedWallPlate",
        "SetAttachedAnchor",
        "SetAttachedWallPlateAnchor",
    ] {
        for value in invalid_ids() {
            let body = json!({"command":command,"id":0,"extent":[1,1],"anchor":{"Point":value}});
            assert!(parse_edit_command(&body).err().unwrap().contains("anchor"));
        }
        for value in [0_u32, u32::MAX] {
            let body = json!({"command":command,"id":0,"extent":[1,1],"anchor":{"Point":value}});
            assert!(parse_edit_command(&body).is_ok());
        }
    }
    for section in invalid_ids().into_iter().filter(|v| !v.is_null()) {
        let body = json!({"command":"AddAttachedSlab","extent":[1,1],"anchor":{"Point":0},"plate":{"method":"TriTrapezoid","section":section}});
        assert!(parse_edit_command(&body).err().unwrap().contains("plate"));
    }
}
