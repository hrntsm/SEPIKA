use super::*;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, FrameSectionUse, Haunch, JointKind, LocalAxis, MaterialCategory,
    MemberDetailAttr, MemberJoint, Node, Section,
};

#[test]
fn source_story_empty_table_mcp_kind_edit_preserves_empty_table_and_undo() {
    let mut model = sepika_io::stbridge::import_stbridge(r#"<ST_BRIDGE version="2.0.2"><StbModel><StbNodes><StbNode id="1" X="0" Y="0" Z="0"/></StbNodes><StbStories><StbStory id="1" name="基部" height="0" kind="GENERAL"/></StbStories></StbModel></ST_BRIDGE>"#).unwrap();
    model.source_stories.clear();
    let initial = model.clone();
    let directory = std::env::temp_dir().join(format!("sepika-497-empty-{}", std::process::id()));
    let mut state = ServerState::with_fs_store(model, &directory).unwrap();
    assert!(apply_edit(&mut state, &serde_json::json!({"command":"SetStoryLevelKind", "story":0, "level_kind":{"Penthouse":{"k":0.7}}})).unwrap().applied);
    assert!(state.model.source_stories.is_empty());
    assert!(state.model.source_stories_initialized);
    assert!(!sepika_io::stbridge::export_stbridge(&state.model)
        .unwrap()
        .contains("<StbStory "));
    state.undo.undo(&mut state.model);
    assert!(state.model.eq_ignoring_dofmap(&initial));
    state.undo.redo(&mut state.model);
    assert!(state.model.source_stories.is_empty());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn source_story_native_kind_mcp_matches_gui_command_and_preserves_imported_kinds() {
    use sepika_core::ids::StoryId;
    use sepika_core::model::{SourceStoryKind, StoryLevelKind};
    let xml = r#"<ST_BRIDGE version="2.0.2"><StbModel><StbNodes><StbNode id="1" X="0" Y="0" Z="0"/><StbNode id="2" X="0" Y="0" Z="3000"/></StbNodes><StbStories><StbStory id="1" name="基部" height="0" kind="GENERAL"/><StbStory id="2" name="上階" height="3000" kind="ROOF"/></StbStories></StbModel></ST_BRIDGE>"#;
    for native in [true, false] {
        let mut model = sepika_io::stbridge::import_stbridge(xml).unwrap();
        if native {
            model.source_stories.clear();
            model.source_stories_initialized = false;
        }
        let initial = model.clone();
        let directory =
            std::env::temp_dir().join(format!("sepika-497-kind-{}-{native}", std::process::id()));
        let mut state = ServerState::with_fs_store(model.clone(), &directory).unwrap();
        let payload = serde_json::json!({"command":"SetStoryLevelKind", "story":1, "level_kind":{"Penthouse":{"k":0.7}}});
        assert!(apply_edit(&mut state, &payload).unwrap().applied);
        let mut undo = UndoStack::new();
        assert!(undo.run(
            &mut model,
            Box::new(sepika_edit::SetStoryLevelKind {
                story: StoryId(1),
                level_kind: StoryLevelKind::Penthouse { k: 0.7 }
            })
        ));
        assert!(model.eq_ignoring_dofmap(&state.model));
        let expected = if native {
            SourceStoryKind::Penthouse
        } else {
            SourceStoryKind::Roof
        };
        assert_eq!(state.model.source_stories[1].kind, expected);
        assert_eq!(state.model.source_stories[1].kind_from_native, native);
        let exported = sepika_io::stbridge::export_stbridge(&state.model).unwrap();
        assert_eq!(
            sepika_io::stbridge::import_stbridge(&exported)
                .unwrap()
                .source_stories[1]
                .kind,
            expected
        );
        state.undo.undo(&mut state.model);
        assert!(state.model.eq_ignoring_dofmap(&initial));
        state.undo.redo(&mut state.model);
        assert_eq!(
            sepika_io::stbridge::export_stbridge(&state.model).unwrap(),
            exported
        );
        let revision = state.undo.revision();
        assert!(!apply_edit(&mut state, &payload).unwrap().applied);
        assert!(!apply_edit(&mut state, &serde_json::json!({"command":"SetStoryLevelKind", "story":4294967295_u32, "level_kind":"Normal"})).unwrap().applied);
        assert_eq!(state.undo.revision(), revision);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn source_story_mcp_edit_query_diagnostics_and_snapshot_match_shared_command() {
    let mut model = sample_model();
    model.assign_stb_node_ids().unwrap();
    model.source_stories.push(sepika_core::model::SourceStory {
        kind_from_native: false,
        id: 51,
        guid: None,
        name: "原階".into(),
        height: 3000.0,
        kind: sepika_core::model::SourceStoryKind::General,
        id_dependence: None,
        strength_concrete: Some("FC27".into()),
        node_ids: vec![sepika_core::model::SourceStoryNode {
            id: 2,
            node: Some(NodeId(1)),
        }],
    });
    let snapshot = model.clone();
    let directory =
        std::env::temp_dir().join(format!("sepika-source-story-mcp-{}", std::process::id()));
    let mut state = ServerState::with_fs_store(model.clone(), &directory).unwrap();
    let payload =
        serde_json::json!({"command":"SetSourceStoryNodes", "source_story":51, "nodes":[0]});
    assert!(apply_edit(&mut state, &payload).unwrap().applied);
    let mut stack = UndoStack::new();
    assert!(stack.run(
        &mut model,
        Box::new(sepika_edit::SetSourceStoryNodes {
            source_story: 51,
            nodes: vec![NodeId(0)]
        })
    ));
    assert!(model.eq_ignoring_dofmap(&state.model));
    assert_eq!(
        query_model(&state.model, "source_stories", None)[0]["node_ids"][0]["id"],
        1
    );
    let expected: Vec<_> = state
        .model
        .source_story_diagnostics()
        .into_iter()
        .chain(state.model.source_story_assignment_diagnostics())
        .map(|message| serde_json::json!({"message":message}))
        .collect();
    assert_eq!(
        query_model(&state.model, "source_story_diagnostics", None),
        expected
    );
    assert_ne!(state.model.source_stories, snapshot.source_stories);
    assert_eq!(snapshot.source_stories[0].node_ids[0].id, 2);
    let error = apply_edit(&mut state, &serde_json::json!({"command":"SetSourceStoryNodes", "source_story":51, "nodes":[4294967295_u32]})).unwrap_err();
    assert_eq!(error, "原階所属には実在する構造節点が必要です");
    assert_eq!(state.undo.revision(), 1);
    state.undo.undo(&mut state.model);
    assert!(state.model.eq_ignoring_dofmap(&snapshot));
    std::fs::remove_dir_all(directory).unwrap();
}

fn sample_model() -> Model {
    Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: sepika_core::dof::Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, 3000.0],
                restraint: sepika_core::dof::Dof6Mask::FREE,
                mass: None,
                story: Some(sepika_core::ids::StoryId(0)),
                support_spring: None,
            },
        ],
        sections: vec![Section {
            frame_use: Some(FrameSectionUse::Column),
            id: SectionId(0),
            name: "H-400".to_string(),
            area: 100.0,
            iy: 1000.0,
            iz: 2000.0,
            j: 50.0,
            depth: 400.0,
            width: 200.0,
            as_y: 0.0,
            as_z: 0.0,
            floor: None,
            panel_thickness: None,
            thickness: None,
            shape: None,
            material: Some(MaterialId(0)),
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: Default::default(),
        }],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [
                sepika_core::model::EndCondition::Fixed,
                sepika_core::model::EndCondition::Fixed,
            ],
            force_regime: sepika_core::model::ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        }],
        ..Default::default()
    }
}

#[test]
fn pending_geometry_queries_and_quantity_are_not_reported_as_zero() {
    use sepika_core::model::{Material, PropertyBasis};
    use sepika_core::section_shape::SectionShape;
    let mut model = sample_model();
    let mut section = SectionShape::SteelH {
        height: 400.0,
        width: 200.0,
        web_thick: 9.0,
        flange_thick: 12.0,
        root_r: None,
    }
    .input_section(SectionId(0), "未入力H".into())
    .unwrap();
    section.material = Some(MaterialId(0));
    section.frame_use = Some(FrameSectionUse::Column);
    model.sections[0] = section;
    model.materials.push(Material {
        id: MaterialId(0),
        name: "SN400B".into(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.0),
        concrete_class: Default::default(),
        strength_factor: None,
    });
    let rows = query_model(&model, "sections", None);
    assert!(rows[0]["area"].is_null());
    assert!(rows[0]["iy"].is_null());
    assert!(rows[0]["unavailable_reason"]
        .as_str()
        .unwrap()
        .contains("未算定"));
    let unavailable = quantity_takeoff_json(&model, None);
    assert_eq!(unavailable["status"], "unavailable");
    assert!(unavailable["totals"].is_null());
    model.sections[0].area = 9000.0;
    model.sections[0].property_basis.area = PropertyBasis::Supplied;
    assert_eq!(query_model(&model, "sections", None)[0]["area"], 9000.0);
    let available = quantity_takeoff_json(&model, None);
    assert!(available["totals"]["steel_t"].as_f64().unwrap() > 0.0);
    let mut known = model.sections[0].clone();
    known.id = SectionId(1);
    known.shape = Some(SectionShape::SteelH {
        height: 400.0,
        width: 200.0,
        web_thick: 9.0,
        flange_thick: 12.0,
        root_r: Some(13.0),
    });
    known.property_basis = Default::default();
    model.sections.push(known);
    let mut normal = model.elements[0].clone();
    normal.section = Some(SectionId(1));
    model.elements[0].id = ElemId(1);
    model.elements.insert(0, normal);
    let before = model.clone();
    match compute_job(&model, JobKind::LinearStatic, &JobParams::default()) {
        Err(error) => assert!(error.to_string().contains("フィレット"), "{error}"),
        Ok(_) => panic!("未知フィレットの部分入力から成功結果を公開してはいけない"),
    }
    assert_eq!(model.load_cases, before.load_cases);
    assert_eq!(model.stories, before.stories);
    assert_eq!(model.nodes, before.nodes);
}

#[test]
fn rounded_cft_jobs_publish_unavailable_reasons_without_claiming_pass() {
    use sepika_core::section_shape::SectionShape;
    let mut model = rc_column_model();
    let mut steel = model.materials[0].clone();
    steel.id = MaterialId(model.materials.len() as u32);
    steel.name = "SN400B".into();
    steel.category = MaterialCategory::Steel;
    steel.fc = None;
    steel.fy = Some(235.0);
    steel.young = 205000.0;
    steel.poisson = 0.3;
    steel.density = 7.85e-9;
    let steel_id = steel.id;
    model.materials.push(steel);
    let mut section = SectionShape::CftBox {
        height: 400.0,
        width: 300.0,
        thick: 10.0,
        corner_r: Some(30.0),
    }
    .to_section(SectionId(0), "角丸CFT".into());
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = Some(MaterialId(0));
    section.steel_material = Some(steel_id);
    model.sections[0] = section;
    match compute_job(&model, JobKind::DesignCheck, &JobParams::default()).unwrap() {
        JobOutcome::DesignCheck { summary, .. } => {
            assert!(summary["n_skipped"].as_u64().unwrap() > 0);
            assert_eq!(summary["all_members_checked_and_ok"], false);
            assert!(summary["member_skipped"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["reason"].as_str().unwrap().contains("#418")));
        }
        _ => panic!("断面検定の結果が必要"),
    }
    match compute_job(&model, JobKind::UltimateCheck, &JobParams::default()).unwrap() {
        JobOutcome::UltimateCheck { summary } => {
            assert_eq!(summary["n_cft_mu_unavailable"], 1);
            let row = &summary["cft_members"][0];
            assert!(row["mu_nm"].is_null());
            assert!(row["mu_nm_unavailable_reason"]
                .as_str()
                .unwrap()
                .contains("#418"));
            assert!(row["ncu"].as_f64().unwrap() > 0.0);
            assert!(!row["axial_ok"].is_null());
            assert_eq!(summary["all_checks_calculated_and_ok"], false);
        }
        _ => panic!("終局検定の結果が必要"),
    }
}

#[test]
fn linear_static_job_rejects_ex_without_seismic_horizontal_load() {
    use sepika_core::ids::LoadCaseId;
    use sepika_core::model::{
        LoadCase, LoadCaseKind, LoadTransfer, RegionAnchor, Slab, SlabPlate, SlabShape,
        SlabTipLoad, TipLoadDirection,
    };

    let mut model = rc_column_model();
    model.load_cases.clear();
    model.nodes[1].coord = [3000.0, 0.0, 0.0];
    model.sections[0].frame_use = Some(FrameSectionUse::Girder);
    model.load_cases.push(LoadCase {
        id: LoadCaseId(0),
        name: sepika_core::model::EX_CASE_NAME.into(),
        kind: LoadCaseKind::Seismic,
        nodal: Vec::new(),
        member: Vec::new(),
    });
    model.slabs.push(Slab {
        id: sepika_core::ids::SlabId(0),
        shape: SlabShape::Attached {
            anchor: RegionAnchor::Line {
                nodes: [NodeId(0), NodeId(1)],
                span: [0.0, 1.0],
                transfer: LoadTransfer::Anchor,
            },
            extent: [1500.0, 1500.0],
        },
        plate: SlabPlate::default(),
        tip_loads: vec![SlabTipLoad {
            case: LoadCaseId(0),
            intensity: 2.0,
            direction: TipLoadDirection::PosX,
        }],
    });
    model.validate().expect("有効な先端荷重モデル");
    let error = job::compute_job(
        &model,
        JobKind::LinearStatic,
        &job::JobParams {
            ai_mode: sepika_solver::statics::analysis::AiMode::SemiPrecise,
            ..Default::default()
        },
    )
    .err()
    .expect("水平力欠損で停止する");
    assert!(error.to_string().contains("地震水平力"), "{error}");
}

#[test]
fn test_query_model_nodes() {
    let m = sample_model();
    let items = query_model(&m, "node", None);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["id"], 0);
    assert_eq!(items[1]["story"], 0);
}

#[test]
fn test_query_model_elements_and_sections() {
    let m = sample_model();
    assert_eq!(query_model(&m, "member", None).len(), 1);
    let secs = query_model(&m, "section", None);
    assert_eq!(secs.len(), 1);
    assert_eq!(secs[0]["name"], "H-400");
}

/// 断面の問い合わせは材料 4 欄を出す。材料は断面が持ち、未割当は解析前チェックが
/// 止めるため、どの断面のどの欄が空かを問い合わせ側から追えるようにする。
#[test]
fn test_query_model_sections_expose_materials() {
    let mut m = sample_model();
    m.sections[0].rebar_material = None;
    let secs = query_model(&m, "section", None);
    assert_eq!(secs[0]["material"], 0, "主材料の ID を出す");
    assert!(
        secs[0]["rebar_material"].is_null(),
        "未割当の欄は null で見分けられる"
    );
    for key in ["floor", "shear_rebar_material", "steel_material"] {
        assert!(secs[0].get(key).is_some(), "{key} の欄がある");
    }
}

#[test]
fn test_query_model_sections_expose_frame_use() {
    let mut m = sample_model();
    let mut beam = m.sections[0].clone();
    beam.id = SectionId(1);
    beam.frame_use = Some(FrameSectionUse::Girder);
    let mut brace = beam.clone();
    brace.id = SectionId(2);
    brace.frame_use = Some(FrameSectionUse::Brace);
    let mut unset = beam.clone();
    unset.id = SectionId(3);
    unset.frame_use = None;
    m.sections.extend([beam, brace, unset]);

    let sections = query_model(&m, "sections", None);
    assert_eq!(sections[0]["frame_use"], "Column");
    assert_eq!(sections[1]["frame_use"], "Girder");
    assert_eq!(sections[2]["frame_use"], "Brace");
    assert!(sections[3]["frame_use"].is_null());
}

#[test]
fn test_query_model_filter() {
    let m = sample_model();
    // 名前で絞り込み（断面名 H-400 を含むものだけ）。
    assert_eq!(query_model(&m, "section", Some("H-400")).len(), 1);
    assert_eq!(query_model(&m, "section", Some("RC")).len(), 0);
}

#[test]
fn test_query_model_unknown_kind() {
    let m = sample_model();
    assert!(query_model(&m, "bogus", None).is_empty());
}

/// 部材付帯情報（ハンチ・継手位置）が登録された部材は、`query_model` の
/// member/elements 出力に `haunch_i`/`haunch_j`/`joints` が含まれる。
/// 付帯情報がない部材（本テストには含めない）は従来どおりのフィールドのみとなる
/// （`test_query_model_elements_and_sections` で確認済み）。
#[test]
fn test_query_model_elements_with_member_detail() {
    let mut m = sample_model();
    m.member_detail_attrs.push(MemberDetailAttr {
        elem: ElemId(0),
        haunch_i: Some(Haunch {
            length: 700.0,
            depth_increase: 200.0,
            width_increase: 0.0,
        }),
        haunch_j: Some(Haunch {
            length: 500.0,
            depth_increase: 150.0,
            width_increase: 50.0,
        }),
        joints: vec![MemberJoint {
            distance: 1000.0,
            kind: JointKind::Shop,
        }],
    });
    let items = query_model(&m, "elements", None);
    assert_eq!(items.len(), 1);
    let e = &items[0];
    assert_eq!(e["haunch_i"]["length"], 700.0);
    assert_eq!(e["haunch_i"]["depth_increase"], 200.0);
    assert_eq!(e["haunch_j"]["width_increase"], 50.0);
    let joints = e["joints"].as_array().expect("joints 配列");
    assert_eq!(joints.len(), 1);
    assert_eq!(joints[0]["distance"], 1000.0);
    assert_eq!(joints[0]["kind"], "Shop");
}

/// RC 矩形の片持ち柱モデル（終局検定ジョブ用）。長期荷重ケース 1 つ。
pub(crate) fn rc_column_model() -> Model {
    use sepika_core::model::{LoadCase, Material, NodalLoad};
    use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};

    let rebar = RcRectColumnRebar {
        main_dia: 25.0,
        x: vec![8],
        y: vec![8],
        cover: 40.0,
        hoop: RectColumnHoop {
            dia: 10.0,
            pitch: 100.0,
            legs_x: 2,
            legs_y: 2,
        },
    };
    let shape = SectionShape::RcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar,
    };
    Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: sepika_core::dof::Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, 3000.0],
                restraint: sepika_core::dof::Dof6Mask::FREE,
                mass: None,
                story: Some(sepika_core::ids::StoryId(0)),
                support_spring: None,
            },
        ],
        // 材料は断面が持つ。RC 断面は主筋・せん断補強筋も要る。
        sections: vec![Section {
            material: Some(MaterialId(0)),
            rebar_material: Some(MaterialId(1)),
            shear_rebar_material: Some(MaterialId(1)),
            frame_use: Some(FrameSectionUse::Column),
            ..shape.to_section(SectionId(0), "C600".into())
        }],
        materials: vec![
            Material {
                strength_factor: None,
                concrete_class: Default::default(),
                id: MaterialId(0),
                name: "Fc24".into(),
                category: MaterialCategory::Concrete,
                young: 23000.0,
                poisson: 0.2,
                density: 2.4e-9,
                shear: None,
                fc: Some(24.0),
                fy: None,
            },
            Material {
                strength_factor: None,
                concrete_class: Default::default(),
                id: MaterialId(1),
                name: "SD345".into(),
                category: MaterialCategory::Rebar,
                young: 205000.0,
                poisson: 0.3,
                density: 7.85e-9,
                shear: None,
                fc: None,
                fy: Some(345.0),
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [
                sepika_core::model::EndCondition::Fixed,
                sepika_core::model::EndCondition::Fixed,
            ],
            force_regime: sepika_core::model::ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        }],
        load_cases: vec![LoadCase {
            kind: Default::default(),
            id: sepika_core::ids::LoadCaseId(0),
            name: "長期".into(),
            nodal: vec![NodalLoad::manual(
                NodeId(1),
                [0.0, 0.0, -500_000.0, 0.0, 0.0, 0.0],
            )],
            member: Vec::new(),
        }],
        ..Default::default()
    }
}

#[test]
fn test_compute_ultimate_check_job() {
    let model = rc_column_model();
    let outcome = compute_job(&model, JobKind::UltimateCheck, &JobParams::default())
        .expect("終局検定ジョブは成功するはず");
    match outcome {
        JobOutcome::UltimateCheck { summary } => {
            assert_eq!(summary["kind"], "UltimateCheck");
            assert_eq!(summary["n_checks"], 1);
            // 柱 1 本のせん断余裕度・耐力が算定されている。
            let members = summary["members"].as_array().expect("members 配列");
            assert_eq!(members.len(), 1);
            assert!(members[0]["qsu"].as_f64().unwrap() > 0.0);
            assert!(members[0]["shear_margin"].as_f64().unwrap() > 0.0);
            // CFT 集計キーが存在する（本モデルは CFT 柱なしなので 0）。
            assert_eq!(summary["n_cft_checks"], 0);
            assert!(summary["cft_members"].is_array());
        }
        _ => panic!("expected UltimateCheck outcome"),
    }
}

/// DesignCheck ジョブは既定では危険断面位置（柱フェイス [face=0 につき節点芯]・
/// 中央）の 3 断面のみを検定する（付帯情報なし）。
#[test]
fn test_compute_design_check_job_default_positions() {
    let model = rc_column_model();
    let outcome = compute_job(&model, JobKind::DesignCheck, &JobParams::default())
        .expect("断面検定ジョブは成功するはず");
    match outcome {
        JobOutcome::DesignCheck { summary, .. } => {
            assert_eq!(summary["kind"], "DesignCheck");
            assert_eq!(summary["n_checks"], 3);
        }
        _ => panic!("expected DesignCheck outcome"),
    }
}

/// 部材付帯情報（継手位置）が登録された部材は、継手位置でも断面力が評価され
/// （sepika-element の `eval_sections` 拡張）、DesignCheck の検定位置にも
/// 継手位置が加わる（既定 3 断面 + 継手 1 = 4 検定）。
#[test]
fn test_compute_design_check_job_member_detail_joint() {
    let mut model = rc_column_model();
    // 節点間距離 3000mm の柱に、始端から 1000mm（正規化 1/3）の現場継手を追加する。
    model.member_detail_attrs.push(MemberDetailAttr {
        elem: ElemId(0),
        haunch_i: None,
        haunch_j: None,
        joints: vec![MemberJoint {
            distance: 1000.0,
            kind: JointKind::Site,
        }],
    });
    let outcome = compute_job(&model, JobKind::DesignCheck, &JobParams::default())
        .expect("断面検定ジョブは成功するはず");
    match outcome {
        JobOutcome::DesignCheck {
            member_force_rows,
            summary,
            ..
        } => {
            assert_eq!(summary["kind"], "DesignCheck");
            // 継手位置 1000/3000 の断面力行が追加されている。
            assert!(member_force_rows
                .iter()
                .any(|(_, pos, _)| (pos - 1000.0 / 3000.0).abs() < 1e-6));
            // 継手位置分だけ検定数が増える（3 -> 4）。
            assert_eq!(summary["n_checks"], 4);
        }
        _ => panic!("expected DesignCheck outcome"),
    }
}

#[test]
fn test_job_registry_lifecycle() {
    let mut reg = JobRegistry::new();
    let id = reg.register(JobKind::LinearStatic);
    assert!(matches!(reg.get(&id).unwrap().status, JobStatus::Queued));
    reg.update(&id, JobStatus::Running { progress: 0.5 });
    assert!(matches!(
        reg.get(&id).unwrap().status,
        JobStatus::Running { progress } if (progress - 0.5).abs() < 1e-6
    ));
    reg.update(
        &id,
        JobStatus::Done {
            result_ref: "r1".into(),
        },
    );
    assert!(matches!(
        &reg.get(&id).unwrap().status,
        JobStatus::Done { result_ref } if result_ref == "r1"
    ));
    // 異なる ID は別ジョブ。
    let id2 = reg.register(JobKind::Eigen);
    assert_ne!(id, id2);
    assert!(reg.get("nonexistent").is_none());
}

#[test]
fn test_quantity_takeoff_json_column() {
    let model = rc_column_model();
    // 部位別（既定）: RC 柱 1 本 → 0.6×0.6×3.0 = 1.08 m³。
    let v = quantity_takeoff_json(&model, None);
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["category"], "柱");
    assert!((rows[0]["concrete_m3"].as_f64().unwrap() - 1.08).abs() < 1e-9);
    // 明細: 部材 1 件。合計と注記も返る。
    let detail = quantity_takeoff_json(&model, Some("detail"));
    assert_eq!(detail["rows"].as_array().unwrap().len(), 1);
    assert!(detail["totals"]["rebar_t"].as_f64().unwrap() > 0.0);
    assert!(!detail["notes"].as_array().unwrap().is_empty());
    // 鉄筋径別: D25（主筋）と D10（フープ）。
    let rebar = quantity_takeoff_json(&model, Some("rebar"));
    assert_eq!(rebar["rows"].as_array().unwrap().len(), 2);
}

#[test]
fn test_query_model_wall_plates() {
    use sepika_core::model::{WallPlate, WallPlateShape};

    let mut m = sample_model();
    m.wall_plates.push(WallPlate {
        dl_support: None,
        self_weight_shares: Vec::new(),
        id: sepika_core::ids::WallPlateId(0),
        shape: WallPlateShape::Enclosed,
        section: Some(SectionId(0)),
        opening_area: 0.0,
        opening_weight: 0.0,
        openings: Vec::new(),
        loads: vec![],
        slit: Default::default(),
    });
    let items = query_model(&m, "wall_plate", None);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], 0);
    assert_eq!(items[0]["shape"]["kind"], "Enclosed");
    // 耐震スリットは辺ごとの真偽値として出す。
    assert_eq!(
        items[0]["slit"],
        serde_json::json!({ "column_face": [false, false], "beam_face": [false, false] })
    );
    // 壁エレメントになるかも出す（どの壁版が解析に効いているかを引けるようにする）。
    assert_eq!(items[0]["becomes_element"], serde_json::json!(false));
}

/// 耐震スリットは辺ごとに読み書きでき、省略時はどの辺も切れていない扱いになる。
#[test]
fn test_apply_edit_set_wall_plate_slit() {
    use sepika_core::model::{WallPlate, WallPlateShape};

    let mut state = ServerState {
        model: sample_model(),
        undo: sepika_edit::UndoStack::new(),
        jobs: JobRegistry::new(),
        results: sepika_io::results::FsResultStore::open(
            std::env::temp_dir().join(format!("sepika-test-{}/mcp_slit_test", std::process::id())),
        )
        .expect("temp store"),
    };
    state.model.wall_plates.push(WallPlate {
        dl_support: None,
        self_weight_shares: Vec::new(),
        id: sepika_core::ids::WallPlateId(0),
        shape: WallPlateShape::Enclosed,
        section: Some(SectionId(0)),
        opening_area: 0.0,
        opening_weight: 0.0,
        openings: Vec::new(),
        loads: vec![],
        slit: Default::default(),
    });

    // 三方スリット（柱際 2 辺 ＋ 下辺）を辺の組み合わせで指定する。
    let body = serde_json::json!({
        "command": "SetWallPlateAttrs",
        "id": 0,
        "dl_support": "UpperBeam",
        "slit": { "column_face": [true, true], "beam_face": [true, false] }
    });
    assert!(apply_edit(&mut state, &body).expect("apply").applied);
    assert_eq!(
        state.model.wall_plates[0].dl_support,
        Some(sepika_core::model::WallDlSupport::UpperBeam)
    );
    assert_eq!(state.model.wall_plates[0].slit.column_face, [true, true]);
    assert_eq!(state.model.wall_plates[0].slit.beam_face, [true, false]);

    // 片方のキーだけでも指定できる。欠けた側は切れていない扱い。
    let body = serde_json::json!({
        "command": "SetWallPlateAttrs",
        "id": 0,
        "slit": { "beam_face": [false, true] }
    });
    assert!(apply_edit(&mut state, &body).expect("apply").applied);
    assert_eq!(state.model.wall_plates[0].slit.column_face, [false, false]);
    assert_eq!(state.model.wall_plates[0].slit.beam_face, [false, true]);

    // 省略すると「どの辺も切れていない」へ戻る（他の属性と同じ既定の扱い）。
    let body = serde_json::json!({ "command": "SetWallPlateAttrs", "id": 0 });
    assert!(apply_edit(&mut state, &body).expect("apply").applied);
    assert!(!state.model.wall_plates[0].slit.any());

    // 要素数が違う配列は誤りとして弾く（片側だけ指定して残りが既定になる、
    // という黙った解釈をしない）。
    let body = serde_json::json!({
        "command": "SetWallPlateAttrs",
        "id": 0,
        "slit": { "column_face": [true] }
    });
    assert!(apply_edit(&mut state, &body).is_err());
}

#[test]
fn test_apply_edit_wall_plate_region_assignment() {
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{NodeId, WallPlateId};
    use sepika_core::model::{Node, WallPlate, WallPlateShape};

    let mut model = Model {
        nodes: (0..4)
            .map(|i| Node {
                id: NodeId(i),
                coord: match i {
                    0 => [0.0, 0.0, 0.0],
                    1 => [3000.0, 0.0, 0.0],
                    2 => [3000.0, 0.0, 3000.0],
                    _ => [0.0, 0.0, 3000.0],
                },
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        sections: sample_model().sections,
        ..Default::default()
    };
    let first = model.add_enclosed_wall_plate_from_nodes(
        &[NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
        WallPlate {
            dl_support: None,
            self_weight_shares: Vec::new(),
            id: WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: None,
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: Vec::new(),
            loads: vec![],
            slit: Default::default(),
        },
    );
    let region = model
        .wall_plate_assignment_region(first)
        .expect("割当領域")
        .id;
    let mut state = ServerState {
        model,
        undo: sepika_edit::UndoStack::new(),
        jobs: JobRegistry::new(),
        results: sepika_io::results::FsResultStore::open(std::env::temp_dir().join(format!(
            "sepika-test-{}/mcp_wall_assign",
            std::process::id()
        )))
        .expect("temp store"),
    };

    // 未設定へ戻すと版も消える。
    let body = serde_json::json!({ "command": "UnsetWallPlateRegion", "region": region.0 });
    assert!(apply_edit(&mut state, &body).expect("apply").applied);
    assert!(state.model.wall_plates.is_empty());

    // 割り当て直すと生成される。
    let body = serde_json::json!({
        "command": "AssignWallPlateToRegion",
        "region": region.0,
        "section": null,
        "opening_area": 0.0,
        "opening_weight": 0.0
    });
    assert!(apply_edit(&mut state, &body).expect("apply").applied);
    assert_eq!(state.model.wall_plates.len(), 1);
    assert!(matches!(
        state.model.wall_plates[0].shape,
        WallPlateShape::Enclosed
    ));
    assert_eq!(query_model(&state.model, "wall_plate", None).len(), 1);

    // 版なしへ。
    let body = serde_json::json!({ "command": "SetWallPlateRegionNoPlate", "region": region.0 });
    assert!(apply_edit(&mut state, &body).expect("apply").applied);
    assert!(state.model.wall_plates.is_empty());

    // 領域不在は applied:false。
    let body = serde_json::json!({ "command": "AssignWallPlateToRegion", "region": 99 });
    assert!(!apply_edit(&mut state, &body).expect("parse ok").applied);
}

#[test]
fn test_apply_edit_noop_unknown_node() {
    let mut state = ServerState {
        model: sample_model(),
        undo: sepika_edit::UndoStack::new(),
        jobs: JobRegistry::new(),
        results: sepika_io::results::FsResultStore::open(
            std::env::temp_dir().join(format!("sepika-test-{}/mcp_edit_noop", std::process::id())),
        )
        .expect("temp store"),
    };
    let body = serde_json::json!({
        "command": "AssignWallPlateToRegion",
        "region": 99
    });
    let result = apply_edit(&mut state, &body).expect("parse ok");
    assert!(!result.applied);
    assert!(state.model.wall_plates.is_empty());
}

#[test]
fn test_query_model_slabs_and_floor_regions() {
    use sepika_core::ids::{FloorRegionId, NodeId, SlabId};
    use sepika_core::model::{DistributionMethod, Slab, SlabPlate, SlabShape};

    let mut m = sample_model();
    m.slabs.push(Slab {
        id: SlabId(0),
        shape: SlabShape::Enclosed,
        plate: SlabPlate {
            section: Some(SectionId(0)),
            method: DistributionMethod::TriTrapezoid,
            ..Default::default()
        },
        tip_loads: Vec::new(),
    });
    m.floor_regions.push(sepika_core::model::FloorRegion {
        id: FloorRegionId(0),
        name: "R1".into(),
        boundary: vec![NodeId(0), NodeId(1)],
        secondary_beams: Vec::new(),
        slab_ids: vec![SlabId(0)],
    });
    assert_eq!(query_model(&m, "slab", None).len(), 1);
    let regions = query_model(&m, "floor_region", None);
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0]["name"], "R1");
}

#[test]
fn test_apply_edit_assign_slab_to_floor_plate_region() {
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, NodeId};
    use sepika_core::model::{
        ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Node, PlateAssignment,
        SlabShape,
    };

    let mut model = Model {
        nodes: (0..4)
            .map(|i| Node {
                id: NodeId(i),
                coord: match i {
                    0 => [0.0, 0.0, 0.0],
                    1 => [3000.0, 0.0, 0.0],
                    2 => [3000.0, 3000.0, 0.0],
                    _ => [0.0, 3000.0, 0.0],
                },
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        sections: sample_model().sections,
        ..Default::default()
    };
    for (i, (a, b)) in [(0u32, 1u32), (1, 2), (2, 3), (3, 0)]
        .into_iter()
        .enumerate()
    {
        model.elements.push(ElementData {
            id: ElemId(i as u32),
            kind: ElementKind::Beam,
            nodes: [NodeId(a), NodeId(b)].into_iter().collect(),
            section: None,
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        });
    }
    let report = model.rebuild_floor_assignment_regions();
    assert_eq!(report.regions, 1, "正方形の大梁で 1 面");
    let region = model.floor_assignment_regions.regions[0].id;

    let mut state = ServerState {
        model,
        undo: sepika_edit::UndoStack::new(),
        jobs: JobRegistry::new(),
        results: sepika_io::results::FsResultStore::open(std::env::temp_dir().join(format!(
            "sepika-test-{}/mcp_edit_assign_slab",
            std::process::id()
        )))
        .expect("temp store"),
    };

    let body = serde_json::json!({
        "command": "AssignSlabToFloorPlateRegion",
        "region": region.0,
        "section": null,
        "method": "TriTrapezoid"
    });
    let result = apply_edit(&mut state, &body).expect("apply");
    assert!(result.applied);
    assert_eq!(state.model.slabs.len(), 1);
    assert!(matches!(state.model.slabs[0].shape, SlabShape::Enclosed));
    assert!(matches!(
        state.model.floor_assignment_regions.regions[0].assignment,
        PlateAssignment::Plate(_)
    ));
}

fn four_node_edit_state(name: &str) -> ServerState {
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::NodeId;
    use sepika_core::model::Node;

    ServerState {
        model: Model {
            nodes: (0..4)
                .map(|i| Node {
                    id: NodeId(i),
                    coord: match i {
                        0 => [0.0, 0.0, 0.0],
                        1 => [3000.0, 0.0, 0.0],
                        2 => [3000.0, 3000.0, 0.0],
                        _ => [0.0, 3000.0, 0.0],
                    },
                    restraint: Dof6Mask::FREE,
                    mass: None,
                    story: None,
                    support_spring: None,
                })
                .collect(),
            elements: if name.starts_with("attached_slab") {
                use sepika_core::ids::*;
                use sepika_core::model::*;
                vec![ElementData {
                    id: ElemId(0),
                    kind: ElementKind::Beam,
                    nodes: vec![NodeId(0), NodeId(1)].into(),
                    section: Some(SectionId(0)),
                    local_axis: LocalAxis {
                        ref_vector: [0.0, 0.0, 1.0],
                    },
                    end_cond: [EndCondition::Fixed; 2],
                    force_regime: ForceRegime::Auto,
                    rigid_zone: Default::default(),
                    plastic_zone: None,
                    spring: None,
                }]
            } else {
                Vec::new()
            },
            sections: sample_model().sections,
            ..Default::default()
        },
        undo: sepika_edit::UndoStack::new(),
        jobs: JobRegistry::new(),
        results: sepika_io::results::FsResultStore::open(std::env::temp_dir().join(format!(
            "sepika-test-{}/mcp_edit_{name}",
            std::process::id()
        )))
        .expect("temp store"),
    }
}

#[test]
fn test_apply_edit_nested_body_wrapper() {
    let mut state = four_node_edit_state("nested_body");
    let body = serde_json::json!({
        "body": {
            "command": "AddAttachedWallPlate",
            "anchor": {
                "Line": { "nodes": [0, 1], "span": [0.0, 1.0], "transfer": "Anchor" }
            },
            "extent": [900.0, 900.0],
            "section": null
        }
    });
    let result = apply_edit(&mut state, &body).expect("apply");
    assert!(result.applied);
    assert_eq!(state.model.wall_plates.len(), 1);
}

#[test]
fn test_apply_edit_add_attached_slab_flat_and_plate() {
    use sepika_core::model::SlabShape;

    let mut state = four_node_edit_state("attached_slab_flat");
    let flat = serde_json::json!({
        "command": "AddAttachedSlab",
        "anchor": {
            "Line": {
                "nodes": [0, 1],
                "span": [0.0, 1.0],
                "transfer": "Anchor"
            }
        },
        "extent": [1000.0, 1000.0],
        "section": 0
    });
    let result = apply_edit(&mut state, &flat).expect("flat");
    assert!(
        result.applied,
        "フラット引数で AddAttachedSlab が適用される"
    );
    assert_eq!(state.model.slabs.len(), 1);
    assert!(matches!(
        state.model.slabs[0].shape,
        SlabShape::Attached { .. }
    ));
    assert_eq!(state.model.slabs[0].plate.section.map(|s| s.0), Some(0));

    let mut state = four_node_edit_state("attached_slab_plate");
    let nested_plate = serde_json::json!({
        "command": "AddAttachedSlab",
        "anchor": {
            "Line": {
                "nodes": [0, 1],
                "span": [0.0, 1.0],
                "transfer": "Anchor"
            }
        },
        "extent": [800.0, 800.0],
        "plate": {
            "section": 0,
            "loads": [],
            "method": "TriTrapezoid"
        }
    });
    let result = apply_edit(&mut state, &nested_plate).expect("plate");
    assert!(
        result.applied,
        "plate オブジェクトでも AddAttachedSlab が適用される"
    );
    assert_eq!(state.model.slabs.len(), 1);
}

#[test]
fn test_apply_edit_add_attached_wall_plate() {
    use sepika_core::model::WallPlateShape;

    let mut state = four_node_edit_state("attached_wall");
    let body = serde_json::json!({
        "command": "AddAttachedWallPlate",
        "anchor": {
            "Line": {
                "nodes": [0, 1],
                "span": [0.0, 1.0],
                "transfer": "Anchor"
            }
        },
        "extent": [1200.0, 1200.0],
        "section": 0
    });
    let result = apply_edit(&mut state, &body).expect("apply");
    assert!(result.applied);
    assert_eq!(state.model.wall_plates.len(), 1);
    assert!(matches!(
        state.model.wall_plates[0].shape,
        WallPlateShape::Attached { .. }
    ));
}

#[test]
fn test_apply_edit_set_floor_region_name() {
    use sepika_core::ids::{FloorRegionId, NodeId, SlabId};

    let mut state = four_node_edit_state("floor_region_name");
    state
        .model
        .floor_regions
        .push(sepika_core::model::FloorRegion {
            id: FloorRegionId(0),
            name: "old".into(),
            boundary: vec![NodeId(0), NodeId(1)],
            secondary_beams: Vec::new(),
            slab_ids: vec![SlabId(0)],
        });
    let body = serde_json::json!({
        "command": "SetFloorRegionName",
        "id": 0,
        "name": "R1"
    });
    let result = apply_edit(&mut state, &body).expect("apply");
    assert!(result.applied);
    assert_eq!(state.model.floor_regions[0].name, "R1");
}

fn expect_parse_err(value: serde_json::Value) -> String {
    match parse_edit_command(&value) {
        Ok(_) => panic!("エラーになるはずだった: {value}"),
        Err(e) => e,
    }
}

#[test]
fn test_parse_rejects_obsolete_set_slab_secondary_beam_ids() {
    let err = expect_parse_err(serde_json::json!({
        "command": "SetSlabSecondaryBeamIds",
        "floor_region": 0,
        "secondary_beam_ids": [1, 2]
    }));
    assert!(err.contains("廃止"), "{err}");
}

#[test]
fn test_parse_requires_secondary_beams_array() {
    let err = expect_parse_err(serde_json::json!({
        "command": "SetFloorRegionSecondaryBeams",
        "floor_region": 0
    }));
    assert!(err.contains("secondary_beams"), "{err}");
}

#[test]
fn test_parse_rejects_legacy_secondary_beam_ids_key() {
    let err = expect_parse_err(serde_json::json!({
        "command": "SetFloorRegionSecondaryBeams",
        "floor_region": 0,
        "secondary_beam_ids": [1, 2]
    }));
    assert!(err.contains("廃止"), "{err}");
}

#[test]
fn test_parse_requires_wall_region_posts() {
    let err = expect_parse_err(serde_json::json!({
        "command": "SetWallRegionPosts",
        "wall_region": 0
    }));
    assert!(err.contains("posts"), "{err}");
}

#[test]
fn test_parse_set_secondary_member_end_support() {
    let cmd = parse_edit_command(&serde_json::json!({
        "command": "SetSecondaryMemberEndSupport",
        "member": 0,
        "end_support": ["Supported", "Free"]
    }))
    .expect("解析できる");
    assert_eq!(cmd.label(), "二次部材の端部支持条件変更");
}

#[test]
fn test_parse_requires_end_support() {
    let err = expect_parse_err(serde_json::json!({
        "command": "SetSecondaryMemberEndSupport",
        "member": 0
    }));
    assert!(err.contains("end_support"), "{err}");
}

/// 廃止した手入力小梁ラインのコマンドは、黙って無視せず明示エラーにする（§3.4 F1）。
#[test]
fn test_parse_rejects_obsolete_set_floor_region_beams() {
    let err = expect_parse_err(serde_json::json!({
        "command": "SetFloorRegionBeams",
        "id": 0,
        "beams": []
    }));
    assert!(err.contains("廃止"), "{err}");
}

#[test]
fn test_parse_requires_unassigned_beam_body() {
    let err = expect_parse_err(serde_json::json!({
        "command": "AddUnassignedBeam"
    }));
    assert!(err.contains("beam"), "{err}");
}

#[test]
fn test_parse_place_secondary_member() {
    let cmd = parse_edit_command(&serde_json::json!({
        "command": "PlaceSecondaryMember",
        "parent": "floor",
        "region": 0,
        "kind": "Beam",
        "ends": {"Supported": [
            {"support": {"Primary": 0}, "position": 0.5},
            {"support": {"Primary": 2}, "position": 0.5}
        ]},
        "name": "J0"
    }))
    .expect("解析できる");
    assert_eq!(cmd.label(), "二次部材配置");
}

#[test]
fn test_parse_requires_place_secondary_member_parent() {
    let err = expect_parse_err(serde_json::json!({
        "command": "PlaceSecondaryMember",
        "kind": "Beam",
        "ends": {"Detached": [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]]}
    }));
    assert!(err.contains("parent"), "{err}");
}

#[test]
fn test_parse_set_secondary_member_ends() {
    let cmd = parse_edit_command(&serde_json::json!({
        "command": "SetSecondaryMemberEnds",
        "member": 0,
        "ends": {"Supported": [
            {"support": {"Primary": 0}, "position": 0.25},
            {"support": {"Primary": 2}, "position": 0.75}
        ]}
    }))
    .expect("解析できる");
    assert_eq!(cmd.label(), "二次部材の端部移動");
}

/// 二次部材の配置は小梁を床領域へ入れ、割当領域を再構築する（MCP 経路）。
#[test]
fn test_apply_edit_place_secondary_member() {
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, FloorRegionId, NodeId};
    use sepika_core::model::{
        ElementData, ElementKind, EndCondition, FloorRegion, ForceRegime, LocalAxis, Node,
    };

    let mut state = four_node_edit_state("place_secondary_member");
    let mut model = Model::default();
    for i in 0..4u32 {
        model.nodes.push(Node {
            id: NodeId(i),
            coord: match i {
                0 => [0.0, 0.0, 0.0],
                1 => [3000.0, 0.0, 0.0],
                2 => [3000.0, 3000.0, 0.0],
                _ => [0.0, 3000.0, 0.0],
            },
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    for (i, (a, b)) in [(0u32, 1u32), (1, 2), (2, 3), (3, 0)]
        .into_iter()
        .enumerate()
    {
        model.elements.push(ElementData {
            id: ElemId(i as u32),
            kind: ElementKind::Beam,
            nodes: [NodeId(a), NodeId(b)].into_iter().collect(),
            section: None,
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        });
    }
    model.floor_regions.push(FloorRegion {
        id: FloorRegionId(0),
        name: String::new(),
        boundary: vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
        secondary_beams: Vec::new(),
        slab_ids: Vec::new(),
    });
    model.rebuild_floor_assignment_regions();
    state.model = model;

    let body = serde_json::json!({
        "command": "PlaceSecondaryMember",
        "parent": "floor",
        "region": 0,
        "kind": "Beam",
        "ends": {"Supported": [
            {"support": {"Primary": 0}, "position": 0.5},
            {"support": {"Primary": 2}, "position": 0.5}
        ]},
        "name": "J0"
    });
    let result = apply_edit(&mut state, &body).expect("apply");
    assert!(result.applied);
    assert_eq!(state.model.beams().count(), 1);
    assert_eq!(state.model.floor_assignment_regions.regions.len(), 2);
    assert!(
        state.model.validate().is_ok(),
        "{:?}",
        state.model.validate()
    );
}

/// MCP 経由で間柱の端部負担率を指定できる。
#[test]
fn test_mcp_set_post_gravity_end_shares() {
    let mut model = sample_model();
    model
        .unassigned_posts
        .push(sepika_core::model::SecondaryMember {
            id: sepika_core::ids::SecondaryMemberId(0),
            gravity_end_shares: None,
            kind: sepika_core::model::SecondaryMemberKind::Post,
            ends: sepika_core::model::SecondaryMemberEnds::Detached([
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 3000.0],
            ]),
            section: None,
            name: "P1".into(),
        });
    let cmd = crate::edit::parse_edit_command(&serde_json::json!({
        "command": "SetPostGravityEndShares", "member": 0, "shares": [0.25, 0.75]
    }))
    .unwrap();
    cmd.apply(&mut model);
    assert_eq!(
        model.unassigned_posts[0].gravity_end_shares,
        Some([0.25, 0.75])
    );
    crate::edit::parse_edit_command(&serde_json::json!({
        "command": "SetPostGravityEndShares", "member": 0, "shares": null
    }))
    .unwrap()
    .apply(&mut model);
    assert_eq!(model.unassigned_posts[0].gravity_end_shares, None);
}

#[test]
fn attached_slab_mcp_rejects_creation_extent_and_anchor_with_common_diagnostic() {
    use sepika_core::ids::{NodeId, SlabId};
    use sepika_core::model::{LoadTransfer, RegionAnchor, Slab, SlabPlate, SlabShape};
    let mut state = four_node_edit_state("attached_slab_invalid");
    let create = serde_json::json!({"command":"AddAttachedSlab","anchor":{"Line":{"nodes":[0,1],"span":[0.25,0.75],"transfer":"Anchor"}},"extent":[1000.0,-1000.0]});
    let invalid = Slab {
        id: SlabId(0),
        shape: SlabShape::Attached {
            anchor: RegionAnchor::Line {
                nodes: [NodeId(0), NodeId(1)],
                span: [0.25, 0.75],
                transfer: LoadTransfer::Anchor,
            },
            extent: [1000.0, -1000.0],
        },
        plate: SlabPlate::default(),
        tip_loads: vec![],
    };
    let expected = state
        .model
        .validate_attached_slab(&invalid)
        .expect_err("attached slab must be rejected")
        .to_string();
    assert_eq!(apply_edit(&mut state, &create).unwrap_err(), expected);
    assert!(state.model.slabs.is_empty());
    assert_eq!(state.undo.revision(), 0);
    let mut create = create;
    create["extent"] = serde_json::json!([1000.0, 2000.0]);
    assert!(apply_edit(&mut state, &create).unwrap().applied);
    let original = format!("{:?}", state.model);
    let revision = state.undo.revision();
    assert_eq!(
        apply_edit(
            &mut state,
            &serde_json::json!({"command":"SetAttachedExtent","id":0,"extent":[1000.0,-1000.0]})
        )
        .unwrap_err(),
        expected
    );
    assert_eq!(format!("{:?}", state.model), original);
    assert_eq!(state.undo.revision(), revision);
    let mut candidate = state.model.slabs[0].clone();
    candidate.shape = SlabShape::Attached {
        anchor: RegionAnchor::Point(NodeId(99)),
        extent: [1000.0, 2000.0],
    };
    let expected = state
        .model
        .validate_attached_slab(&candidate)
        .expect_err("attached slab must be rejected")
        .to_string();
    assert_eq!(
        apply_edit(
            &mut state,
            &serde_json::json!({"command":"SetAttachedAnchor","id":0,"anchor":{"Point":99}})
        )
        .unwrap_err(),
        expected
    );
    assert_eq!(format!("{:?}", state.model), original);
    assert_eq!(state.undo.revision(), revision);
}

#[test]
fn loaded_full_length_intent_survives_mcp_preparation_and_linear_analysis() {
    use sepika_core::ids::LoadCaseId;
    use sepika_core::model::{LoadCase, LoadCaseKind, MemberLoad};
    let mut model = rc_column_model();
    let elem = model.elements[0].id;
    let length_mm = model.member_length(&model.elements[0]);
    let case_id = LoadCaseId(model.load_cases.len() as u32);
    model.load_cases.push(LoadCase {
        id: case_id,
        name: "手入力".into(),
        kind: LoadCaseKind::Other,
        nodal: vec![],
        member: vec![MemberLoad::full_length_uniform(
            elem,
            [0.0, 0.0, -1.0],
            length_mm,
            10.0,
        )],
    });
    let path = std::env::temp_dir().join(format!("sepika-449-mcp-{}.ovika", std::process::id()));
    sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
    let loaded = sepika_io::ovika::load_ovika(&path).unwrap().model;
    let params = JobParams {
        load_case: Some(case_id.0),
        ..Default::default()
    };
    let (prepared, _) = job::model_prepared_for_analysis(&loaded, &params).unwrap();
    assert_eq!(
        prepared.load_cases[case_id.index()],
        model.load_cases[case_id.index()]
    );
    let outcome = compute_job(&loaded, JobKind::LinearStatic, &params)
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(matches!(outcome, JobOutcome::LinearStatic { case, .. } if case == case_id.0));
    assert_eq!(
        loaded.load_cases[case_id.index()],
        model.load_cases[case_id.index()]
    );
    let mut invalid = loaded;
    if let sepika_core::model::MemberLoadKind::Distributed { b, .. } =
        &mut invalid.load_cases[case_id.index()].member[0].kind
    {
        *b += 100.0;
    }
    match compute_job(&invalid, JobKind::LinearStatic, &params) {
        Err(error) => assert!(error.to_string().contains("member[0]")),
        Ok(_) => panic!("全長属性と不整合な作用区間を解析してはいけない"),
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn wall_horizontal_input_survives_ovika_and_mcp_design_reports_missing_assignment() {
    use sepika_core::model::LoadCaseKind;
    use sepika_core::section_shape::SectionShape;
    let mut model = rc_column_model();
    let mut bottom = model.nodes[0].clone();
    bottom.id = NodeId(2);
    bottom.coord = [4000.0, 0.0, 0.0];
    let mut top = model.nodes[1].clone();
    top.id = NodeId(3);
    top.coord = [4000.0, 0.0, 3000.0];
    model.nodes.extend([bottom, top]);
    let mut girder = model.sections[0].clone();
    girder.id = SectionId(1);
    girder.name = "梁".into();
    girder.frame_use = Some(FrameSectionUse::Girder);
    model.sections.push(girder);
    for (id, nodes, section) in [(1, [0, 2], 1), (2, [1, 3], 1), (3, [2, 3], 0)] {
        let mut element = model.elements[0].clone();
        element.id = ElemId(id);
        element.nodes = nodes.map(NodeId).into_iter().collect();
        element.section = Some(SectionId(section));
        element.local_axis.ref_vector = [0.0, 1.0, 0.0];
        model.elements.push(element);
    }
    let mut section = SectionShape::RcWall {
        thickness: 180.0,
        ps: 0.002,
        pwh_ratio: Some(0.006),
    }
    .to_section(SectionId(2), "壁".into());
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = None;
    model.sections.push(section);
    model.add_enclosed_wall_plate_from_nodes(
        &[NodeId(0), NodeId(2), NodeId(3), NodeId(1)],
        sepika_core::model::WallPlate {
            id: sepika_core::ids::WallPlateId(0),
            shape: sepika_core::model::WallPlateShape::Enclosed,
            section: Some(SectionId(2)),
            dl_support: Some(sepika_core::model::WallDlSupport::LowerBeam),
            self_weight_shares: vec![],
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: vec![],
            loads: vec![],
            slit: Default::default(),
        },
    );
    model.wall_weight_generation =
        Some(sepika_core::model::WallWeightGenerationMode::GravityCasesOnly);
    model.load_cases[0].kind = LoadCaseKind::Wind;
    let path = std::env::temp_dir().join(format!("sepika-503-mcp-{}.ovika", std::process::id()));
    sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
    let loaded = sepika_io::ovika::load_ovika(&path).unwrap().model;
    std::fs::remove_file(path).unwrap();
    assert_eq!(loaded.sections[2].shape, model.sections[2].shape);
    let outcome = compute_job(
        &loaded,
        JobKind::DesignCheck,
        &JobParams {
            load_case: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    let JobOutcome::DesignCheck { summary, .. } = outcome else {
        panic!("断面検定結果")
    };
    let skipped = summary["wall_checks"].as_array().unwrap();
    for kind in ["AllowableShear", "ReferenceSkeleton"] {
        let item = skipped
            .iter()
            .find(|item| item["kind"] == kind)
            .expect("不足出力はSkipped");
        assert_eq!(item["plate"], 0);
        assert_eq!(item["case"], "case:0");
        assert_eq!(item["skip_kind"], "MissingInput");
        let reason = item["outcome"]["Skipped"]["reason"].as_str().unwrap();
        assert!(
            reason.contains("耐震壁 ID") && reason.contains("横筋") && reason.contains("未割当"),
            "{reason}"
        );
    }
}

fn circular_post_model() -> sepika_core::model::Model {
    use sepika_core::ids::{MaterialId, NodeId, SecondaryMemberId, SectionId};
    use sepika_core::model::{
        Material, MaterialCategory, Model, Node, SecondaryMember, SecondaryMemberEnds,
        SecondaryMemberKind,
    };
    use sepika_core::section_shape::{CircleColumnHoop, RcCircleColumnRebar, SectionShape};
    let mut section = SectionShape::RcColumnCircle {
        d: 400.0,
        rebar: RcCircleColumnRebar {
            main_dia: 25.0,
            count: 0,
            cover: 40.0,
            hoop: CircleColumnHoop {
                dia: 10.0,
                pitch: 100.0,
            },
        },
    }
    .to_section(SectionId(0), "円形断面".into());
    section.material = Some(MaterialId(0));
    section.frame_use = Some(sepika_core::model::FrameSectionUse::Column);
    section.width = 900.0;
    section.depth = 700.0;
    let ends = [[0.0, 0.0, 0.0], [0.0, 0.0, 3000.0]];
    Model {
        nodes: ends
            .into_iter()
            .enumerate()
            .map(|(id, coord)| Node {
                id: NodeId(id as u32),
                coord,
                restraint: sepika_core::dof::Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        sections: vec![section],
        materials: vec![Material {
            id: MaterialId(0),
            name: "Fc24".into(),
            category: MaterialCategory::Concrete,
            young: 22700.0,
            poisson: 0.2,
            density: 2.4e-9,
            shear: None,
            fc: Some(24.0),
            fy: None,
            concrete_class: Default::default(),
            strength_factor: None,
        }],
        unassigned_posts: vec![SecondaryMember {
            id: SecondaryMemberId(448),
            kind: SecondaryMemberKind::Post,
            ends: SecondaryMemberEnds::Detached(ends),
            section: Some(SectionId(0)),
            name: "間柱符号".into(),
            gravity_end_shares: None,
        }],
        ..Default::default()
    }
}

#[test]
fn circular_post_quantity_headless_preserves_values_and_diagnostics() {
    let mut model = circular_post_model();
    for grouping in ["category", "story", "detail"] {
        let value = quantity_takeoff_json(&model, Some(grouping));
        assert!((value["totals"]["concrete_m3"].as_f64().unwrap() - 0.3769911184).abs() < 1e-9);
        assert!((value["totals"]["formwork_m2"].as_f64().unwrap() - 3.7699111843).abs() < 1e-9);
    }
    for ends in [[[0.0; 3]; 2], [[0.0; 3], [0.0, 0.0, f64::INFINITY]]] {
        model.unassigned_posts[0].ends = sepika_core::model::SecondaryMemberEnds::Detached(ends);
        let value = quantity_takeoff_json(&model, None);
        assert_eq!(value["status"], "unavailable");
        assert!(value["totals"].is_null());
        let reason = value["reason"].as_str().unwrap();
        assert!(reason.contains("SecondaryMemberId(448)"), "{reason}");
        assert!(reason.contains("実長 L"), "{reason}");
    }
}

#[test]
fn stb_strength_mcp_edits_diagnostics_and_undo_use_resolved_material() {
    let model = sepika_io::stbridge::import_stbridge(include_str!(
        "../../sepika-io/tests/fixtures/strength_priority.stb"
    ))
    .unwrap();
    let initial = model.clone();
    let directory = std::env::temp_dir().join(format!("sepika-520-mcp-{}", std::process::id()));
    let mut state = ServerState::with_fs_store(model, &directory).unwrap();
    let mut input = state.model.stb_strengths.clone();
    input.members[0].concrete = None;
    input.sections[0].concrete = None;
    assert!(
        apply_edit(
            &mut state,
            &serde_json::json!({"command":"SetStbStrengths","input":input})
        )
        .unwrap()
        .applied
    );
    assert_eq!(
        state
            .model
            .element_material(&state.model.elements[0])
            .unwrap()
            .fc,
        Some(27.)
    );
    assert!(apply_edit(&mut state,&serde_json::json!({"command":"SetSourceStoryConcreteStrength","source_story":1,"strength":"Fc33"})).unwrap().applied);
    assert_eq!(
        state
            .model
            .element_material(&state.model.elements[0])
            .unwrap()
            .fc,
        Some(33.)
    );
    assert!(!query_model(&state.model, "resolved_strengths", None).is_empty());
    assert!(query_model(&state.model, "strength_diagnostics", None).is_empty());
    state.undo.undo(&mut state.model);
    state.undo.undo(&mut state.model);
    assert!(state.model.eq_ignoring_dofmap(&initial));
    state.undo.redo(&mut state.model);
    state.undo.redo(&mut state.model);
    let mut invalid = state.model.stb_strengths.clone();
    invalid.members[0].concrete = Some("SN400UNKNOWN".into());
    assert!(
        apply_edit(
            &mut state,
            &serde_json::json!({"command":"SetStbStrengths","input":invalid})
        )
        .unwrap()
        .applied
    );
    assert!(state
        .model
        .element_material(&state.model.elements[0])
        .is_none());
    assert!(!query_model(&state.model, "strength_diagnostics", None).is_empty());
    let before = state.model.clone();
    let mut duplicate = before.stb_strengths.clone();
    duplicate.members.push(duplicate.members[0].clone());
    assert!(
        !apply_edit(
            &mut state,
            &serde_json::json!({"command":"SetStbStrengths","input":duplicate})
        )
        .unwrap()
        .applied
    );
    assert!(state.model.eq_ignoring_dofmap(&before));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn stb_strength_native_name_and_numeric_edits_keep_grade_value_consistent() {
    use sepika_core::model::Material;
    use sepika_edit::{
        MaterialField, SectionMaterialRole, SetMaterialField, SetMaterialName, SetSectionMaterial,
    };
    let source = include_str!("../../sepika-io/tests/fixtures/strength_priority.stb");
    let mut model = sepika_io::stbridge::import_stbridge(source).unwrap();
    let initial = model.clone();
    let mut undo = UndoStack::new();
    let id = model.element_material(&model.elements[0]).unwrap().id;
    assert!(undo.run(
        &mut model,
        Box::new(SetMaterialName {
            id,
            name: "Fc99".into()
        })
    ));
    let resolved = model
        .resolve_stb_concrete(&model.stb_strengths.members[0])
        .unwrap();
    assert_eq!((resolved.grade.as_str(), resolved.value), ("Fc36", 36.));
    assert_eq!(
        model.element_material(&model.elements[0]).unwrap().fc,
        Some(36.)
    );
    assert_eq!(
        sepika_io::stbridge::import_stbridge(
            &sepika_io::stbridge::export_stbridge(&model).unwrap()
        )
        .unwrap()
        .stb_strengths
        .members[0]
            .concrete,
        Some("Fc36".into())
    );
    assert!(undo.run(
        &mut model,
        Box::new(SetMaterialField {
            id,
            field: MaterialField::Fc,
            value: Some(33.)
        })
    ));
    assert_eq!(
        model
            .resolve_stb_concrete(&model.stb_strengths.members[0])
            .unwrap()
            .value,
        33.
    );
    assert_eq!(
        model.element_material(&model.elements[0]).unwrap().fc,
        Some(33.)
    );
    undo.undo(&mut model);
    undo.undo(&mut model);
    assert!(model.eq_ignoring_dofmap(&initial));
    let custom_id = MaterialId(model.materials.len() as u32);
    model.materials.push(Material {
        id: custom_id,
        name: "native custom concrete".into(),
        category: MaterialCategory::Concrete,
        young: 21000.,
        poisson: 0.23,
        density: 2.35e-9,
        shear: None,
        fc: Some(39.),
        fy: None,
        strength_factor: None,
        concrete_class: Default::default(),
    });
    let section = model.elements[0].section.unwrap();
    assert!(undo.run(
        &mut model,
        Box::new(SetSectionMaterial {
            section,
            role: SectionMaterialRole::Main,
            material: Some(custom_id)
        })
    ));
    // Explicit member Fc remains above the intentionally changed section assignment.
    assert_eq!(
        model.element_material(&model.elements[0]).unwrap().fc,
        Some(36.)
    );
    let mut input = model.stb_strengths.clone();
    input.members[0].concrete = None;
    assert!(undo.run(&mut model, Box::new(sepika_edit::SetStbStrengths { input })));
    let resolved = model
        .resolve_stb_concrete(&model.stb_strengths.members[0])
        .unwrap();
    assert_eq!(resolved.value, 39.);
    assert!(resolved.native_override);
    assert_eq!(
        model.element_material(&model.elements[0]).unwrap().id,
        custom_id
    );
    assert_eq!(
        model.element_material(&model.elements[0]).unwrap().young,
        21000.
    );
    let out = sepika_io::stbridge::export_stbridge(&model).unwrap();
    let again = sepika_io::stbridge::import_stbridge(&out).unwrap();
    assert_eq!(
        again.element_material(&again.elements[0]).unwrap().fc,
        Some(39.)
    );
    assert!(again.stb_strengths.members[0].concrete.is_none());
    let previous = model.stb_strengths.clone();
    assert!(undo.run(
        &mut model,
        Box::new(SetMaterialField {
            id: custom_id,
            field: MaterialField::Density,
            value: Some(2.4e-9)
        })
    ));
    assert_eq!(previous, model.stb_strengths);
    undo.undo(&mut model);
    undo.undo(&mut model);
    undo.undo(&mut model);
    assert_eq!(
        model.element_material(&model.elements[0]).unwrap().fc,
        Some(36.)
    );
    undo.redo(&mut model);
    undo.redo(&mut model);
    assert_eq!(
        model.element_material(&model.elements[0]).unwrap().fc,
        Some(39.)
    );
    assert!(model.validate().is_ok());
}

#[test]
fn stb_strength_wall_delete_first_middle_last_restores_raw_targets_and_values() {
    use sepika_core::ids::WallPlateId;
    use sepika_core::model::{
        LoadTransfer, RegionAnchor, StbMemberStrength, StrengthTarget, WallPlate, WallPlateShape,
    };
    for deleted in 0..3 {
        let mut model = sepika_io::stbridge::import_stbridge(include_str!(
            "../../sepika-io/tests/fixtures/strength_priority.stb"
        ))
        .unwrap();
        for (index, fc) in [24., 30., 36.].into_iter().enumerate() {
            let id = WallPlateId(index as u32);
            model.wall_plates.push(WallPlate {
                dl_support: None,
                id,
                shape: WallPlateShape::Attached {
                    anchor: RegionAnchor::Line {
                        nodes: [NodeId(0), NodeId(1)],
                        span: [0., 1.],
                        transfer: LoadTransfer::Anchor,
                    },
                    extent: Some([1000., 1000.]),
                },
                section: None,
                opening_area: 0.,
                opening_weight: 0.,
                openings: Vec::new(),
                loads: Vec::new(),
                slit: Default::default(),
                self_weight_shares: Vec::new(),
            });
            model.stb_strengths.members.push(StbMemberStrength {
                target: StrengthTarget::Wall(id),
                node: NodeId(1),
                node_order: vec![NodeId(0), NodeId(1)],
                concrete: Some(format!("Fc{fc}")),
            });
        }
        model.prepare_stb_strength_materials();
        let original = model.clone();
        let mut undo = UndoStack::new();
        assert!(undo.run(
            &mut model,
            Box::new(sepika_edit::DeleteWallPlate {
                id: WallPlateId(deleted)
            })
        ));
        assert!(model.validate().is_ok(), "{:?}", model.validate());
        let expected: Vec<_> = [24., 30., 36.]
            .into_iter()
            .enumerate()
            .filter(|(i, _)| *i != deleted as usize)
            .map(|(_, fc)| fc)
            .collect();
        for (index, plate) in model.wall_plates.iter().enumerate() {
            assert_eq!(plate.id, WallPlateId(index as u32));
            assert_eq!(
                model.wall_plate_material(plate).unwrap().fc,
                Some(expected[index])
            );
        }
        undo.undo(&mut model);
        assert!(model.eq_ignoring_dofmap(&original));
        assert!(model.validate().is_ok());
        undo.redo(&mut model);
        assert!(model.validate().is_ok());
    }
}

#[test]
fn stb_strength_native_rebar_and_shear_assignment_preserve_individual_roles() {
    use sepika_edit::{SectionMaterialRole, SetMaterialName, SetSectionMaterial};
    let mut model = sepika_io::stbridge::import_stbridge(include_str!(
        "../../sepika-io/tests/fixtures/public_strength_bars.stb"
    ))
    .unwrap();
    let section = model.elements[0].section.unwrap();
    let initial = model.clone();
    let mut undo = UndoStack::new();
    let main = model.element_rebar_material(&model.elements[0]).unwrap().id;
    assert!(undo.run(
        &mut model,
        Box::new(SetMaterialName {
            id: main,
            name: "SD390".into()
        })
    ));
    let raw = model
        .stb_strengths
        .sections
        .iter()
        .find(|s| s.section == section)
        .unwrap()
        .reinforcement
        .iter()
        .find(|r| r.part == "main")
        .unwrap();
    assert_eq!(model.resolve_stb_rebar(raw).unwrap().value, 345.);
    assert_eq!(
        model.element_rebar_material(&model.elements[0]).unwrap().fy,
        Some(345.)
    );
    let id = MaterialId(model.materials.len() as u32);
    let props = sepika_core::standard_material::standard_material_properties("SD390").unwrap();
    model.materials.push(sepika_core::model::Material {
        id,
        name: "SD390".into(),
        category: MaterialCategory::Rebar,
        young: props.young,
        poisson: props.poisson,
        density: props.density,
        fc: None,
        fy: Some(390.),
        shear: None,
        strength_factor: None,
        concrete_class: Default::default(),
    });
    assert!(undo.run(
        &mut model,
        Box::new(SetSectionMaterial {
            section,
            role: SectionMaterialRole::Rebar,
            material: Some(id)
        })
    ));
    assert_eq!(
        model.element_rebar_material(&model.elements[0]).unwrap().fy,
        Some(390.)
    );
    assert_eq!(
        model
            .element_shear_rebar_material(&model.elements[0])
            .unwrap()
            .fy,
        Some(295.)
    );
    let shear = initial
        .element_rebar_material(&initial.elements[0])
        .unwrap()
        .id;
    assert!(undo.run(
        &mut model,
        Box::new(SetSectionMaterial {
            section,
            role: SectionMaterialRole::ShearRebar,
            material: Some(shear)
        })
    ));
    assert_eq!(
        model
            .element_shear_rebar_material(&model.elements[0])
            .unwrap()
            .fy,
        Some(345.)
    );
    let again = sepika_io::stbridge::import_stbridge(
        &sepika_io::stbridge::export_stbridge(&model).unwrap(),
    )
    .unwrap();
    assert_eq!(
        again.element_rebar_material(&again.elements[0]).unwrap().fy,
        Some(390.)
    );
    assert_eq!(
        again
            .element_shear_rebar_material(&again.elements[0])
            .unwrap()
            .fy,
        Some(345.)
    );
    undo.undo(&mut model);
    undo.undo(&mut model);
    undo.undo(&mut model);
    assert_eq!(model.stb_strengths, initial.stb_strengths);
    assert_eq!(
        model.element_rebar_material(&model.elements[0]).unwrap().fy,
        Some(345.)
    );
    undo.redo(&mut model);
    undo.redo(&mut model);
    undo.redo(&mut model);
    assert_eq!(
        model.element_rebar_material(&model.elements[0]).unwrap().fy,
        Some(390.)
    );
}

#[test]
fn stb_strength_material_fy_edit_keeps_raw_grade_and_refuses_numeric_loss() {
    use sepika_edit::{MaterialField, SetMaterialField};
    let mut model = sepika_io::stbridge::import_stbridge(include_str!(
        "../../sepika-io/tests/fixtures/public_strength_bars.stb"
    ))
    .unwrap();
    let before = model.clone();
    let id = model.element_rebar_material(&model.elements[0]).unwrap().id;
    let mut undo = UndoStack::new();
    assert!(undo.run(
        &mut model,
        Box::new(SetMaterialField {
            id,
            field: MaterialField::Fy,
            value: Some(390.)
        })
    ));
    let bar = model
        .stb_strengths
        .sections
        .iter()
        .flat_map(|s| &s.reinforcement)
        .find(|r| r.part == "main")
        .unwrap();
    let result = model.resolve_stb_rebar(bar).unwrap();
    assert_eq!(result.grade, "SD345");
    assert_eq!(result.value, 390.);
    assert!(result.native_override);
    assert_eq!(bar.strength, Some("SD345".into()));
    assert_eq!(
        model.element_rebar_material(&model.elements[0]).unwrap().fy,
        Some(390.)
    );
    assert!(matches!(
        sepika_io::stbridge::export_stbridge(&model),
        Err(sepika_io::stbridge::StbError::Unmappable(_))
    ));
    undo.undo(&mut model);
    assert!(model.eq_ignoring_dofmap(&before));
    assert!(sepika_io::stbridge::export_stbridge(&model).is_ok());
    undo.redo(&mut model);
    assert_eq!(
        model
            .resolve_stb_rebar(
                model
                    .stb_strengths
                    .sections
                    .iter()
                    .flat_map(|s| &s.reinforcement)
                    .find(|r| r.part == "main")
                    .unwrap()
            )
            .unwrap()
            .value,
        390.
    );
}

#[test]
fn stb_strength_src_fy_edit_preserves_grade_and_undo_redo() {
    use sepika_edit::{MaterialField, SetMaterialField};
    let mut model = sepika_io::stbridge::import_stbridge(include_str!(
        "../../sepika-io/tests/fixtures/strength_src.stb"
    ))
    .unwrap();
    let original = model.clone();
    let section = model.section(model.elements[0].section.unwrap()).unwrap();
    let id = section.steel_material.unwrap();
    let mut undo = UndoStack::new();
    assert!(undo.run(
        &mut model,
        Box::new(SetMaterialField {
            id,
            field: MaterialField::Fy,
            value: Some(390.),
        })
    ));
    for edited in [true, false, true] {
        let result = model
            .resolve_stb_steel(&model.stb_strengths.sections[0].steel[0])
            .unwrap();
        assert_eq!(result.grade, "SN490B");
        assert_eq!(result.value, if edited { 390. } else { 325. });
        assert_eq!(result.native_override, edited);
        assert_eq!(model.materials[id.index()].fy, Some(result.value));
        if edited {
            assert!(matches!(
                sepika_io::stbridge::export_stbridge(&model),
                Err(sepika_io::stbridge::StbError::Unmappable(_))
            ));
            undo.undo(&mut model);
        } else {
            assert!(model.eq_ignoring_dofmap(&original));
            assert!(sepika_io::stbridge::export_stbridge(&model).is_ok());
            undo.redo(&mut model);
        }
    }
}

#[test]
fn stb_strength_src_steel_assignment_adopts_material_and_restores_input() {
    use sepika_core::ids::MaterialId;
    use sepika_edit::{DeleteMaterial, SectionMaterialRole, SetSectionMaterial};
    let mut model = sepika_io::stbridge::import_stbridge(include_str!(
        "../../sepika-io/tests/fixtures/strength_src.stb"
    ))
    .unwrap();
    let section = model.elements[0].section.unwrap();
    let original_steel = model.section(section).unwrap().steel_material.unwrap();
    let mut replacement = model.materials[original_steel.index()].clone();
    let id = MaterialId(model.materials.len() as u32);
    replacement.id = id;
    replacement.name = "SN400B".into();
    replacement.fy = Some(235.);
    model.materials.push(replacement);
    model.prepare_stb_strength_materials();
    let before = model.clone();
    let mut undo = UndoStack::new();
    assert!(undo.run(
        &mut model,
        Box::new(SetSectionMaterial {
            section,
            role: SectionMaterialRole::Steel,
            material: Some(id),
        })
    ));
    for edited in [true, false, true] {
        let input = &model.stb_strengths.sections[0].steel[0];
        let result = model.resolve_stb_steel(input).unwrap();
        assert_eq!(result.grade, if edited { "SN400B" } else { "SN490B" });
        assert_eq!(result.value, if edited { 235. } else { 325. });
        assert_eq!(result.source, sepika_core::model::StrengthSource::Section);
        assert_eq!(input.native_material, if edited { Some(id) } else { None });
        let adopted = model.section(section).unwrap().steel_material.unwrap();
        assert_eq!(model.materials[adopted.index()].fy, Some(result.value));
        let output = sepika_io::stbridge::export_stbridge(&model).unwrap();
        let again = sepika_io::stbridge::import_stbridge(&output).unwrap();
        assert_eq!(
            again
                .resolve_stb_steel(&again.stb_strengths.sections[0].steel[0])
                .unwrap()
                .value,
            result.value
        );
        if edited {
            assert!(!undo.run(&mut model, Box::new(DeleteMaterial { id })));
            let path = std::env::temp_dir().join(format!(
                "sepika-src-assignment-{}.ovika",
                std::process::id()
            ));
            sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
            let restored = sepika_io::ovika::load_ovika(&path).unwrap().model;
            std::fs::remove_file(path).unwrap();
            assert_eq!(restored.stb_strengths, model.stb_strengths);
            let mut shifted = model.clone();
            let mut extra = shifted.materials[0].clone();
            shifted.visit_material_ids(|id| id.0 += 1);
            extra.id = MaterialId(0);
            shifted.materials.insert(0, extra);
            assert!(shifted.validate().is_ok());
            assert_eq!(
                shifted.stb_strengths.sections[0].steel[0].native_material,
                Some(MaterialId(id.0 + 1))
            );
            assert_eq!(
                shifted
                    .resolve_stb_steel(&shifted.stb_strengths.sections[0].steel[0])
                    .unwrap()
                    .value,
                235.
            );
            undo.undo(&mut model);
        } else {
            assert!(model.eq_ignoring_dofmap(&before));
            undo.redo(&mut model);
        }
    }
    assert!(undo.run(
        &mut model,
        Box::new(SetSectionMaterial {
            section,
            role: SectionMaterialRole::Steel,
            material: None,
        })
    ));
    assert!(model.section(section).unwrap().steel_material.is_none());
    assert!(model
        .resolve_stb_steel(&model.stb_strengths.sections[0].steel[0])
        .is_err());
    assert!(matches!(
        sepika_io::stbridge::export_stbridge(&model),
        Err(sepika_io::stbridge::StbError::Unmappable(_))
    ));
    undo.undo(&mut model);
    assert!(model.eq_ignoring_dofmap(&before));
    undo.redo(&mut model);
    assert!(model
        .resolve_stb_steel(&model.stb_strengths.sections[0].steel[0])
        .is_err());
}

#[test]
fn mcp_eigen_rejects_unknown_wall_band_and_refreshes_valid_edits_before_solving() {
    use sepika_core::model::*;
    let mut model = sample_model();
    let mut column = sepika_core::section_shape::SectionShape::SteelH {
        height: 400.0,
        width: 200.0,
        web_thick: 9.0,
        flange_thick: 13.0,
        root_r: Some(13.0),
    }
    .to_section(SectionId(0), "検証柱".into());
    column.frame_use = Some(FrameSectionUse::Column);
    column.material = Some(MaterialId(0));
    model.sections[0] = column.clone();
    column.id = SectionId(1);
    column.frame_use = Some(FrameSectionUse::Girder);
    model.sections.push(column);
    for (id, coord) in [(2, [4000.0, 0.0, 0.0]), (3, [4000.0, 0.0, 3000.0])] {
        let mut n = model.nodes[if id == 2 { 0 } else { 1 }].clone();
        n.id = NodeId(id);
        n.coord = coord;
        model.nodes.push(n);
    }
    model.materials.push(Material {
        id: MaterialId(0),
        name: "検証鋼".into(),
        category: MaterialCategory::Steel,
        young: 200000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.0),
        concrete_class: Default::default(),
        strength_factor: None,
    });
    for (id, a, b) in [(1, 0, 2), (2, 1, 3), (3, 2, 3)] {
        let mut e = model.elements[0].clone();
        e.id = ElemId(id);
        e.nodes = vec![NodeId(a), NodeId(b)].into();
        e.section = Some(SectionId(if a == 2 { 0 } else { 1 }));
        e.local_axis.ref_vector = [0.0, 1.0, 0.0];
        model.elements.push(e);
    }
    model.add_enclosed_wall_plate_from_nodes(
        &[NodeId(0), NodeId(2), NodeId(3), NodeId(1)],
        WallPlate {
            id: sepika_core::ids::WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: None,
            dl_support: Some(WallDlSupport::LowerBeam),
            self_weight_shares: vec![],
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: vec![WallOpening {
                width: 1000.0,
                height: 500.0,
                offset: Some([1000.0, 2000.0]),
            }],
            loads: vec![AreaLoad {
                kind: "仕上げ".into(),
                value: 0.001,
            }],
            slit: Default::default(),
        },
    );
    let params = crate::job::JobParams::default();
    let error = match crate::job::compute_job(&model, crate::JobKind::Eigen, &params) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("wall generation must be rejected"),
    };
    assert!(
        error.contains("壁版 0") && error.contains("未設定"),
        "{error}"
    );
    let apply = |model: &mut Model| {
        let gen =
            sepika_load::story_gen::generate_stories_with_opts(model, &[], true, model.mass_method)
                .unwrap();
        for (n, s) in model.nodes.iter_mut().zip(&gen.node_story) {
            n.story = *s;
        }
        for rn in gen.rep_nodes {
            if rn.id.index() < model.nodes.len() {
                let index = rn.id.index();
                model.nodes[index] = rn;
            } else {
                model.nodes.push(rn);
            }
        }
        model.stories = gen.stories;
        model.constraints = gen.constraints;
        model.generated_masters = gen.generated_masters;
        model.damper_mass_generation = Some(gen.damper_mass_generation);
        model.wall_weight_generation = Some(gen.wall_weight_generation);
    };
    apply(&mut model);
    if let Err(error) = crate::job::compute_job(&model, crate::JobKind::Eigen, &params) {
        panic!("regenerated eigen: {error}");
    }
    model.wall_plates[0].openings[0].offset = Some([1000.0, 0.0]);
    let error = sepika_job::compute::compute_eigen(model.clone(), 1)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("壁版 0") && error.contains("再生成"),
        "{error}"
    );
    let (prepared, _) = crate::job::model_prepared_for_analysis(&model, &params).unwrap();
    prepared.validate_wall_weight_generation().unwrap();
    assert!(sepika_job::weight_preparation::weights_are_current(
        &prepared,
        prepared.mass_method
    ));
    assert_ne!(
        prepared.stories[0].wall_weights,
        model.stories[0].wall_weights
    );
    let wall_total: f64 = prepared
        .stories
        .iter()
        .flat_map(|s| &s.wall_weights)
        .map(|w| w.band.design_n)
        .sum();
    assert!((wall_total - 11500.0).abs() < 1e-8);
    crate::job::compute_job(&model, crate::JobKind::Eigen, &params).unwrap();
    model = prepared;
    model.wall_plates[0].openings[0].offset = Some([f64::NAN, 0.0]);
    let error = match crate::job::compute_job(&model, crate::JobKind::Eigen, &params) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("invalid opening must be rejected"),
    };
    assert!(
        error.contains("壁版") && error.contains("開口位置"),
        "{error}"
    );
}

fn load_state_contract_model() -> Model {
    use sepika_core::ids::LoadCaseId;
    use sepika_core::model::{LoadCase, LoadCaseKind as K, LoadCombination, NodalLoad};
    let mut model = rc_column_model();
    model.load_cases = [
        (K::Dead, 100.0),
        (K::Live, 20.0),
        (K::Snow, 40.0),
        (K::Seismic, 30.0),
        (K::Seismic, 30.0),
        (K::Wind, 30.0),
        (K::LiveSeismic, 8.0),
        (K::Dead, 900.0),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (kind, kn))| LoadCase {
        id: LoadCaseId(i as u32),
        name: format!("手入力{i}"),
        kind,
        nodal: vec![NodalLoad::manual(
            NodeId(1),
            [kn * 1000.0, 0.0, -kn * 1000.0, 0.0, 0.0, 0.0],
        )],
        member: vec![],
    })
    .collect();
    for extra in [
        vec![],
        vec![(2, 0.7)],
        vec![(2, 1.0)],
        vec![(2, 0.35), (3, 1.0)],
        vec![(2, 0.35), (3, -1.0)],
        vec![(2, 0.35), (4, 1.0)],
        vec![(2, 0.35), (4, -1.0)],
        vec![(5, 1.0)],
        vec![(2, 0.35), (5, 1.0)],
    ] {
        let mut terms = vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)];
        terms.extend(extra.into_iter().map(|(id, f)| (LoadCaseId(id), f)));
        model.combinations.push(LoadCombination {
            name: format!("任意名称{}", model.combinations.len()),
            terms,
        });
    }
    model
}

#[test]
fn load_state_saved_mcp_job_keeps_independent_n_q_m_values_and_duration() {
    let mut model = load_state_contract_model();
    let path = std::env::temp_dir().join(format!("sepika487-mcp-{}.ovika", std::process::id()));
    sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
    model = sepika_io::ovika::load_ovika(&path).unwrap().model;
    std::fs::remove_file(path).unwrap();
    for (index, expected_kn, term) in [
        (0, 120.0, "long"),
        (1, 148.0, "long"),
        (2, 160.0, "short"),
        (3, 164.0, "short"),
        (4, 104.0, "short"),
        (5, 164.0, "short"),
        (6, 104.0, "short"),
        (7, 150.0, "short"),
        (8, 164.0, "short"),
    ] {
        let JobOutcome::DesignCheck {
            member_force_rows,
            summary,
            ..
        } = compute_job(
            &model,
            JobKind::DesignCheck,
            &JobParams {
                load_combination: Some(index),
                ..Default::default()
            },
        )
        .unwrap()
        else {
            panic!("DesignCheck")
        };
        assert_eq!(summary["term"], term);
        assert_eq!(summary["load_target"]["source"], "saved_combination");
        assert_eq!(summary["load_target"]["state"]["combination"], true);
        assert_eq!(summary["load_target"]["legal_conditions_verified"], false);
        let (_, _, f) = member_force_rows
            .iter()
            .find(|(_, p, _)| *p == 0.0)
            .unwrap();
        assert!(
            (f[0].abs() / 1000.0 - expected_kn).abs() < 1e-8,
            "index {index}: {f:?}"
        );
        assert!((f[1].hypot(f[2]) / 1000.0 - expected_kn).abs() < 1e-8);
        assert!((f[4].hypot(f[5]) / 1_000_000.0 - 3.0 * expected_kn).abs() < 1e-8);
        if index == 3 {
            assert_eq!(
                summary["load_target"]["gravity_reference_terms"],
                serde_json::json!([[0, 1.0], [1, 1.0], [2, 0.35]])
            );
        }
    }
    let terms = model.combinations[0].terms.clone();
    model.combinations[0].name = "LOADCASE 雪 地震".into();
    let JobOutcome::DesignCheck { summary, .. } = compute_job(
        &model,
        JobKind::DesignCheck,
        &JobParams {
            load_combination: Some(0),
            ..Default::default()
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(summary["term"], "long");
    assert_eq!(summary["load_target"]["terms"], serde_json::json!(terms));
    model.combinations[0].terms = model.combinations[2].terms.clone();
    let JobOutcome::DesignCheck { summary, .. } = compute_job(
        &model,
        JobKind::DesignCheck,
        &JobParams {
            load_combination: Some(0),
            ..Default::default()
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(summary["term"], "short");
}

#[test]
fn load_state_mcp_single_unknown_and_invalid_target_are_never_passed() {
    let model = load_state_contract_model();
    for id in [2, 5, 6] {
        let JobOutcome::DesignCheck { summary, .. } = compute_job(
            &model,
            JobKind::DesignCheck,
            &JobParams {
                load_case: Some(id),
                ..Default::default()
            },
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(summary["all_checked_and_ok"], false);
        assert!(summary["n_skipped"].as_u64().unwrap() > 0);
        assert_eq!(summary["load_target"]["source"], "single_case");
    }
    for params in [
        JobParams {
            load_case: Some(0),
            load_combination: Some(0),
            ..Default::default()
        },
        JobParams {
            load_combination: Some(99),
            ..Default::default()
        },
        JobParams {
            load_case: Some(3),
            ..Default::default()
        },
    ] {
        assert!(matches!(
            compute_job(&model, JobKind::DesignCheck, &params),
            Err(sepika_job::JobError::InvalidInput(_))
        ));
    }
}

#[test]
fn load_state_mcp_automatic_gravity_uses_frame_live_and_rejects_any_failed_gravity() {
    use sepika_core::model::{LoadCaseKind as K, MemberLoad, MemberLoadExtent, MemberLoadKind};
    let mut model = load_state_contract_model();
    model.load_cases[2].kind = K::Other;
    model.load_cases[7].kind = K::Other;
    let params = JobParams {
        load_case: Some(3),
        ..Default::default()
    };
    let JobOutcome::DesignCheck {
        member_force_rows,
        summary,
        ..
    } = compute_job(&model, JobKind::DesignCheck, &params).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        summary["load_target"]["source"],
        "automatic_gravity_combination"
    );
    let (_, _, f) = member_force_rows
        .iter()
        .find(|(_, p, _)| *p == 0.0)
        .unwrap();
    let baseline_n = f[0].abs();
    assert!(summary["load_target"]["terms"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t[0] == 1));
    assert!(!summary["load_target"]["terms"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t[0] == 6));
    let mut changed = model.clone();
    changed.load_cases[1].nodal[0].values[2] -= 12_000.0;
    changed.load_cases[6].nodal[0].values[2] -= 92_000.0;
    let JobOutcome::DesignCheck {
        member_force_rows, ..
    } = compute_job(&changed, JobKind::DesignCheck, &params).unwrap()
    else {
        panic!()
    };
    let (_, _, f) = member_force_rows
        .iter()
        .find(|(_, p, _)| *p == 0.0)
        .unwrap();
    assert!((f[0].abs() - baseline_n - 12_000.0).abs() < 1e-8);
    for failed in [vec![1usize], vec![0usize, 1usize]] {
        let mut bad = model.clone();
        for index in failed {
            let mut load = MemberLoad::manual(
                ElemId(0),
                [0.0, 0.0, -1.0],
                MemberLoadKind::Point {
                    a: 100.0,
                    p: 1000.0,
                },
            );
            load.extent = MemberLoadExtent::FullLengthUniform;
            bad.load_cases[index].member.push(load);
        }
        assert!(
            compute_job(&bad, JobKind::DesignCheck, &params).is_err(),
            "重力一部/全部失敗時は拒否"
        );
    }
}

#[test]
fn load_state_rc_girder_missing_q0_is_skipped_instead_of_fem_ql_substitution() {
    let mut model = load_state_contract_model();
    model.nodes[1].coord = [3000.0, 0.0, 0.0];
    model.elements[0].local_axis.ref_vector = [0.0, 0.0, 1.0];
    model.sections[0].frame_use = Some(FrameSectionUse::Girder);
    let params = JobParams {
        load_combination: Some(3),
        ..Default::default()
    };
    let JobOutcome::DesignCheck { summary, .. } =
        compute_job(&model, JobKind::DesignCheck, &params).unwrap()
    else {
        panic!()
    };
    assert_eq!(summary["all_checked_and_ok"], false);
    assert!(summary["member_skipped"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["reason"].as_str().unwrap().contains("QD1用")));
}
fn beam_contact_fixture() -> sepika_core::model::Model {
    use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId, SlabId};
    use sepika_core::model::*;
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};
    let mut beam = SectionShape::RcBeamRect {
        b: 300.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: 0.0,
            top: vec![],
            bottom: vec![],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 0.0,
                pitch: 0.0,
                legs: 0,
            },
        },
    }
    .to_section(SectionId(0), "G451".into());
    beam.material = Some(MaterialId(0));
    beam.frame_use = Some(FrameSectionUse::Girder);
    let mut sections = vec![beam];
    for (id, t) in [(1, 150.0), (2, 100.0)] {
        let mut section =
            SectionShape::RcSlab { thickness: t }.to_section(SectionId(id), "床".into());
        section.material = Some(MaterialId(0));
        sections.push(section);
    }
    Model {
        nodes: [[0.0, 0.0, 3000.0], [6000.0, 0.0, 3000.0]]
            .into_iter()
            .enumerate()
            .map(|(id, coord)| Node {
                id: NodeId(id as u32),
                coord,
                restraint: sepika_core::dof::Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            plastic_zone: None,
            spring: None,
        }],
        sections,
        materials: vec![Material {
            id: MaterialId(0),
            name: "Fc24".into(),
            category: MaterialCategory::Concrete,
            young: 22700.0,
            poisson: 0.2,
            density: 2.4e-9,
            shear: None,
            fc: Some(24.0),
            fy: None,
            concrete_class: Default::default(),
            strength_factor: None,
        }],
        slabs: [(0, 1, 2000.0), (1, 2, -2000.0)]
            .into_iter()
            .map(|(id, sec, extent)| Slab {
                id: SlabId(id),
                shape: SlabShape::Attached {
                    anchor: RegionAnchor::Line {
                        nodes: [NodeId(0), NodeId(1)],
                        span: [0.0, 1.0],
                        transfer: LoadTransfer::Anchor,
                    },
                    extent: [extent; 2],
                },
                plate: SlabPlate {
                    section: Some(SectionId(sec)),
                    ..Default::default()
                },
                tip_loads: vec![],
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn beam_contact_quantity_mcp_preserves_union_and_diagnostics() {
    for model in beam_contact_narrow_fixtures() {
        let value = quantity_takeoff_json(&model, Some("detail"));
        let beam = value["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["label"] == "G451")
            .unwrap();
        assert!(
            (beam["formwork_m2"].as_f64().unwrap() - 8.1).abs() <= 1e-9,
            "{value}"
        );
    }
    let no_plate = quantity_takeoff_json(&beam_contact_no_plate_fixture(), Some("detail"));
    let beam = no_plate["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["label"] == "G451")
        .unwrap();
    assert!(
        (beam["formwork_m2"].as_f64().unwrap() - 8.4).abs() <= 1e-9,
        "{no_plate}"
    );
    let model = beam_contact_fixture();
    let value = quantity_takeoff_json(&model, Some("detail"));
    let items = value["rows"].as_array().unwrap();
    let beam = items.iter().find(|i| i["label"] == "G451").unwrap();
    assert!(
        (beam["formwork_m2"].as_f64().unwrap() - 7.5).abs() <= 1e-9,
        "{value}"
    );
    for (invalid, expected_reason) in beam_contact_invalid_cases() {
        let value = quantity_takeoff_json(&invalid, Some("detail"));
        assert_eq!(value["status"], "unavailable");
        assert!(value["totals"].is_null());
        assert!(
            value["reason"].as_str().unwrap().contains(expected_reason),
            "{value}"
        );
    }
}

fn beam_contact_invalid_cases() -> Vec<(sepika_core::model::Model, &'static str)> {
    use sepika_core::ids::{FloorPlateAssignmentRegionId, NodeId, SectionId, SlabId};
    use sepika_core::model::{
        FloorPlateAssignmentRegion, PlateAssignment, RegionAnchor, SlabShape,
    };
    let base = beam_contact_fixture();
    let mut cases = Vec::new();
    let mut model = base.clone();
    let mut midpoint = model.nodes[0].clone();
    midpoint.id = NodeId(2);
    midpoint.coord[0] = 3000.0;
    model.nodes.push(midpoint);
    model.elements[0].nodes = [NodeId(0), NodeId(2), NodeId(1)].into_iter().collect();
    for section in &mut model.sections[1..] {
        section.thickness = Some(600.0);
        section.shape = Some(sepika_core::section_shape::SectionShape::RcSlab { thickness: 600.0 });
    }
    let mut boundary = model.elements[0].clone();
    boundary.id = sepika_core::ids::ElemId(1);
    boundary.nodes = [NodeId(0), NodeId(1)].into_iter().collect();
    boundary.section = None;
    model.elements.push(boundary);
    assert!(model.validate().is_ok());
    for _ in 0..2 {
        cases.push((model.clone(), "Primary(ElemId(0)): 2節点以外"));
        model.elements[0].nodes.reverse();
    }

    let mut model = base.clone();
    model.elements[0].section = Some(SectionId(999));
    cases.push((model, "Primary(ElemId(0)): 梁断面 SectionId(999)"));
    let mut model = base.clone();
    for i in 0..2 {
        let mut node = model.nodes[i].clone();
        node.id = NodeId(i as u32 + 2);
        node.coord[2] = 3100.0;
        model.nodes.push(node);
    }
    for slab in &mut model.slabs {
        if let SlabShape::Attached {
            anchor: RegionAnchor::Line { nodes, .. },
            ..
        } = &mut slab.shape
        {
            *nodes = [NodeId(2), NodeId(3)];
        }
    }
    model.nodes[1].coord[2] += 0.5;
    cases.push((model.clone(), "高さが一定でない梁"));
    model.elements[0].nodes.reverse();
    cases.push((model, "高さが一定でない梁"));

    for t in [-1.0, 0.0, f64::NAN, f64::INFINITY] {
        let mut model = base.clone();
        model.sections[1].thickness = Some(t);
        cases.push((model, "実厚"));
    }
    for assignment in [PlateAssignment::Unset, PlateAssignment::Plate(SlabId(999))] {
        let mut model = base.clone();
        model
            .floor_assignment_regions
            .regions
            .push(FloorPlateAssignmentRegion {
                id: FloorPlateAssignmentRegionId(451),
                boundary: vec![],
                assignment,
            });
        cases.push((
            model,
            if assignment.is_unset() {
                "Unset"
            } else {
                "床板"
            },
        ));
    }
    let mut model = base.clone();
    model.slabs[0].plate.section = Some(SectionId(999));
    cases.push((model, "断面"));
    let mut model = base.clone();
    if let SlabShape::Attached {
        anchor: RegionAnchor::Line { nodes, .. },
        ..
    } = &mut model.slabs[0].shape
    {
        nodes[0] = NodeId(999);
    }
    cases.push((model, "参照"));
    let mut model = base.clone();
    model.sections[0].shape = None;
    cases.push((model, "矩形"));
    let mut model = base.clone();
    if let SlabShape::Attached { extent, .. } = &mut model.slabs[0].shape {
        *extent = [2000.0, -2000.0];
    }
    cases.push((model, "自己交差"));
    let mut model = base.clone();
    for i in 0..2 {
        let mut node = model.nodes[i].clone();
        node.id = NodeId(i as u32 + 2);
        model.nodes.push(node);
    }
    for slab in &mut model.slabs {
        if let SlabShape::Attached {
            anchor: RegionAnchor::Line { nodes, .. },
            ..
        } = &mut slab.shape
        {
            *nodes = [NodeId(2), NodeId(3)];
        }
    }
    model.nodes[0].coord[2] = 0.0;
    cases.push((model.clone(), "傾斜梁"));
    model.elements[0].nodes.reverse();
    cases.push((model, "傾斜梁"));
    cases
}

fn beam_contact_narrow_fixtures() -> Vec<sepika_core::model::Model> {
    let mut models = Vec::new();
    for side in [-1.0, 1.0] {
        let mut model = beam_contact_fixture();
        model.slabs.truncate(1);
        if let sepika_core::model::SlabShape::Attached { extent, .. } = &mut model.slabs[0].shape {
            *extent = [side * 5.0; 2];
        }
        models.push(model.clone());
        model.elements[0].nodes.reverse();
        models.push(model);
    }
    models
}

fn beam_contact_no_plate_fixture() -> sepika_core::model::Model {
    let mut model = beam_contact_fixture();
    model.slabs.remove(0);
    model
        .floor_assignment_regions
        .regions
        .push(sepika_core::model::FloorPlateAssignmentRegion {
            id: sepika_core::ids::FloorPlateAssignmentRegionId(451),
            boundary: vec![],
            assignment: sepika_core::model::PlateAssignment::NoPlate,
        });
    model
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn beam_contact_quantity_mcp_tool_returns_verified_rows_and_unavailable_reasons() {
    use crate::server::{QuantityTakeoffArgs, SepikaServer};
    use rmcp::handler::server::wrapper::Parameters;
    let mut cases = vec![
        (beam_contact_fixture(), Some(7.5), ""),
        (beam_contact_no_plate_fixture(), Some(8.4), ""),
    ];
    cases.extend(
        beam_contact_narrow_fixtures()
            .into_iter()
            .map(|model| (model, Some(8.1), "")),
    );
    cases.extend(
        beam_contact_invalid_cases()
            .into_iter()
            .map(|(model, reason)| (model, None, reason)),
    );
    for (id, (model, expected, reason)) in cases.into_iter().enumerate() {
        let dir = std::env::temp_dir().join(format!(
            "sepika-beam-contact-mcp-{}-{id}",
            std::process::id()
        ));
        let server = SepikaServer::new(ServerState::with_fs_store(model, &dir).unwrap());
        let result = server
            .quantity_takeoff(Parameters(QuantityTakeoffArgs {
                group_by: Some("detail".into()),
            }))
            .await
            .unwrap();
        let text = &result.content[0].raw.as_text().unwrap().text;
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        if let Some(expected) = expected {
            let beam = value["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|i| i["label"] == "G451")
                .unwrap();
            assert!(
                (beam["formwork_m2"].as_f64().unwrap() - expected).abs() <= 1e-9,
                "{value}"
            );
        } else {
            assert_eq!(value["status"], "unavailable");
            assert!(value["totals"].is_null());
            assert!(
                value["reason"].as_str().unwrap().contains(reason),
                "{value}"
            );
        }
        drop(server);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn stb_strength_s_main_assignment_changes_steel_without_concrete_attribute() {
    use sepika_edit::{SectionMaterialRole, SetSectionMaterial};
    let mut model = sepika_io::stbridge::import_stbridge(include_str!(
        "../../sepika-io/tests/fixtures/public_strength_bars.stb"
    ))
    .unwrap();
    let section = model.elements[1].section.unwrap();
    let original = model.section(section).unwrap().material.unwrap();
    let mut material = model.materials[original.index()].clone();
    let id = MaterialId(model.materials.len() as u32);
    material.id = id;
    material.name = "SN490B".into();
    material.fy = Some(325.);
    model.materials.push(material);
    let before = model.clone();
    let mut undo = UndoStack::new();
    assert!(undo.run(
        &mut model,
        Box::new(SetSectionMaterial {
            section,
            role: SectionMaterialRole::Main,
            material: Some(id),
        })
    ));
    for edited in [true, false, true] {
        let input = model
            .stb_strengths
            .sections
            .iter()
            .find(|s| s.section == section)
            .unwrap();
        assert!(input.concrete.is_none());
        let result = model.resolve_stb_steel(&input.steel[0]).unwrap();
        assert_eq!(result.grade, if edited { "SN490B" } else { "SN400B" });
        assert_eq!(result.value, if edited { 325. } else { 235. });
        assert_eq!(
            model.element_material(&model.elements[1]).unwrap().fy,
            Some(result.value)
        );
        let output = sepika_io::stbridge::export_stbridge(&model).unwrap();
        assert!(!output.contains("strength_concrete=\"SN"));
        let again = sepika_io::stbridge::import_stbridge(&output).unwrap();
        assert_eq!(
            again.element_material(&again.elements[1]).unwrap().fy,
            Some(result.value)
        );
        if edited {
            let path =
                std::env::temp_dir().join(format!("sepika-s-main-{}.ovika", std::process::id()));
            sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
            let restored = sepika_io::ovika::load_ovika(&path).unwrap().model;
            std::fs::remove_file(path).unwrap();
            assert_eq!(restored.stb_strengths, model.stb_strengths);
            undo.undo(&mut model);
        } else {
            assert!(model.eq_ignoring_dofmap(&before));
            undo.redo(&mut model);
        }
    }
    assert!(undo.run(
        &mut model,
        Box::new(SetSectionMaterial {
            section,
            role: SectionMaterialRole::Main,
            material: None,
        })
    ));
    assert!(model.element_material(&model.elements[1]).is_none());
    let input = model
        .stb_strengths
        .sections
        .iter()
        .find(|s| s.section == section)
        .unwrap();
    assert!(input.concrete.is_none());
    assert!(model.resolve_stb_steel(&input.steel[0]).is_err());
    assert!(matches!(
        sepika_io::stbridge::export_stbridge(&model),
        Err(sepika_io::stbridge::StbError::Unmappable(_))
    ));
    undo.undo(&mut model);
    assert!(model.eq_ignoring_dofmap(&before));
    undo.redo(&mut model);
    assert!(model.element_material(&model.elements[1]).is_none());
}

#[test]
fn stb_strength_raw_steel_edit_updates_src_s_and_secondary_consumers() {
    use sepika_core::model::{SecondaryMember, SecondaryMemberEnds};
    for (index, xml, member, grade, expected) in [
        (
            0,
            include_str!("../../sepika-io/tests/fixtures/strength_src.stb"),
            0,
            "SN400B",
            235.,
        ),
        (
            1,
            include_str!("../../sepika-io/tests/fixtures/public_strength_bars.stb"),
            1,
            "SN490B",
            325.,
        ),
    ] {
        let mut model = sepika_io::stbridge::import_stbridge(xml).unwrap();
        let section = model.elements[member].section.unwrap();
        if index == 1 {
            model.unassigned_beams.push(SecondaryMember {
                section: Some(section),
                ends: SecondaryMemberEnds::Detached([[1000., 1000., 4700.], [5000., 1000., 4700.]]),
                ..Default::default()
            });
        }
        let before = model.clone();
        let dir = std::env::temp_dir().join(format!(
            "sepika-520-steel-input-{}-{index}",
            std::process::id()
        ));
        let mut state = ServerState::with_fs_store(model, &dir).unwrap();
        let mut input = state.model.stb_strengths.clone();
        input
            .sections
            .iter_mut()
            .find(|s| s.section == section)
            .unwrap()
            .steel[0]
            .strength = grade.into();
        assert!(
            apply_edit(
                &mut state,
                &serde_json::json!({"command":"SetStbStrengths","input":input})
            )
            .unwrap()
            .applied
        );
        let after = state.model.clone();
        for edited in [true, false, true] {
            let raw = &state
                .model
                .stb_strengths
                .sections
                .iter()
                .find(|s| s.section == section)
                .unwrap()
                .steel[0];
            let resolved = state.model.resolve_stb_steel(raw).unwrap();
            let value = if edited {
                expected
            } else if index == 0 {
                325.
            } else {
                235.
            };
            assert_eq!(resolved.value, value);
            assert!(!resolved.native_override);
            let material = if index == 0 {
                state
                    .model
                    .element_steel_material(&state.model.elements[member])
            } else {
                assert_eq!(
                    state
                        .model
                        .secondary_material(&state.model.unassigned_beams[0])
                        .unwrap()
                        .fy,
                    Some(value)
                );
                state.model.element_material(&state.model.elements[member])
            }
            .unwrap();
            assert_eq!(material.fy, Some(value));
            assert!(state.model.stb_strength_diagnostics().is_empty());
            let output = sepika_io::stbridge::export_stbridge(&state.model).unwrap();
            let again = sepika_io::stbridge::import_stbridge(&output).unwrap();
            let supplied = if index == 0 {
                again.element_steel_material(&again.elements[member])
            } else {
                again.element_material(&again.elements[member])
            };
            assert_eq!(supplied.unwrap().fy, Some(value));
            if edited {
                state.undo.undo(&mut state.model);
            } else {
                assert!(state.model.eq_ignoring_dofmap(&before));
                state.undo.redo(&mut state.model);
            }
        }
        assert!(state.model.eq_ignoring_dofmap(&before));
        state.undo.redo(&mut state.model);
        assert!(state.model.eq_ignoring_dofmap(&after));
        drop(state);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn stb_strength_native_concrete_assignment_does_not_generate_materials_and_undo_is_complete() {
    use sepika_edit::{SectionMaterialRole, SetSectionMaterial};
    let mut model = sepika_io::stbridge::import_stbridge(include_str!(
        "../../sepika-io/tests/fixtures/public_strength_bars.stb"
    ))
    .unwrap();
    let section = model.elements[0].section.unwrap();
    let mut material = model
        .materials
        .iter()
        .find(|m| m.fc.is_some())
        .unwrap()
        .clone();
    material.id = MaterialId(model.materials.len() as u32);
    material.name = "Custom Concrete".into();
    material.fc = Some(37.);
    let id = material.id;
    model.materials.push(material);
    let before = model.clone();
    let mut undo = UndoStack::new();
    assert!(undo.run(
        &mut model,
        Box::new(SetSectionMaterial {
            section,
            role: SectionMaterialRole::Main,
            material: Some(id),
        })
    ));
    let after = model.clone();
    assert_eq!(model.materials, before.materials);
    assert_eq!(model.element_material(&model.elements[0]).unwrap().id, id);
    assert_eq!(
        model.element_material(&model.elements[0]).unwrap().fc,
        Some(37.)
    );
    assert!(model.validate().is_ok());
    let output = sepika_io::stbridge::export_stbridge(&model).unwrap();
    let again = sepika_io::stbridge::import_stbridge(&output).unwrap();
    assert_eq!(
        again.element_material(&again.elements[0]).unwrap().fc,
        Some(37.)
    );
    undo.undo(&mut model);
    assert!(model.eq_ignoring_dofmap(&before));
    undo.redo(&mut model);
    assert!(model.eq_ignoring_dofmap(&after));
}

#[test]
fn stb_strength_raw_steel_edit_keeps_explicit_native_override_separate() {
    use sepika_edit::{SectionMaterialRole, SetSectionMaterial, SetStbStrengths};
    let mut model = sepika_io::stbridge::import_stbridge(include_str!(
        "../../sepika-io/tests/fixtures/strength_src.stb"
    ))
    .unwrap();
    let section = model.elements[0].section.unwrap();
    let id = model.element_steel_material(&model.elements[0]).unwrap().id;
    let mut undo = UndoStack::new();
    assert!(undo.run(
        &mut model,
        Box::new(SetSectionMaterial {
            section,
            role: SectionMaterialRole::Steel,
            material: Some(id),
        })
    ));
    let before = model.clone();
    let mut input = model.stb_strengths.clone();
    input.sections[0].steel[0].strength = "SN400B".into();
    assert!(undo.run(&mut model, Box::new(SetStbStrengths { input })));
    let resolved = model
        .resolve_stb_steel(&model.stb_strengths.sections[0].steel[0])
        .unwrap();
    assert_eq!(resolved.grade, "SN400B");
    assert_eq!(resolved.value, 325.);
    assert!(resolved.native_override);
    assert_eq!(
        model.element_steel_material(&model.elements[0]).unwrap().id,
        id
    );
    assert_eq!(
        model.element_steel_material(&model.elements[0]).unwrap().fy,
        Some(325.)
    );
    assert!(matches!(
        sepika_io::stbridge::export_stbridge(&model),
        Err(sepika_io::stbridge::StbError::Unmappable(_))
    ));
    undo.undo(&mut model);
    assert!(model.eq_ignoring_dofmap(&before));
    undo.redo(&mut model);
    assert_eq!(
        model.element_steel_material(&model.elements[0]).unwrap().fy,
        Some(325.)
    );
}

#[test]
fn stb_strength_incompatible_steel_parts_do_not_supply_a_single_material() {
    use sepika_edit::SetStbStrengths;
    let mut model = sepika_io::stbridge::import_stbridge(include_str!(
        "../../sepika-io/tests/fixtures/strength_src.stb"
    ))
    .unwrap();
    let mut input = model.stb_strengths.clone();
    let mut other = input.sections[0].steel[0].clone();
    other.part = "web".into();
    other.strength = "SN400B".into();
    input.sections[0].steel.push(other);
    let mut undo = UndoStack::new();
    assert!(undo.run(&mut model, Box::new(SetStbStrengths { input })));
    assert!(model.element_steel_material(&model.elements[0]).is_none());
    assert!(model
        .stb_strength_diagnostics()
        .iter()
        .any(|reason| reason.contains("部位別鋼材強度")));
}
