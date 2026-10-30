use super::*;
use sepika_app::app::study::Location;
use sepika_core::ids::{ElemId, SectionId};

#[cfg(all(windows, feature = "gui"))]
#[path = "issue985_wire.rs"]
mod wire;

fn fixture_content_hash(bytes: &[u8]) -> String {
    use sha2::Digest;
    let text = std::str::from_utf8(bytes).unwrap().replace("\r\n", "\n");
    format!("{:x}", sha2::Sha256::digest(text.as_bytes()))
}

#[test]
fn issue985_fixture_hash_ignores_checkout_line_endings_but_detects_content_changes() {
    let bytes = std::fs::read(fixture_path()).unwrap();
    let lf = std::str::from_utf8(&bytes).unwrap().replace("\r\n", "\n");
    let expected = "c137797755de759807a3e04f90fa349a01104072045e526ca22b353c25f72e9a";
    assert_eq!(fixture_content_hash(lf.as_bytes()), expected);
    assert_eq!(
        fixture_content_hash(lf.replace('\n', "\r\n").as_bytes()),
        expected
    );
    assert_ne!(fixture_content_hash(format!("{lf} ").as_bytes()), expected);
}

fn fixture() -> App {
    let mut app = imported();
    assert_eq!(
        fixture_content_hash(&std::fs::read(fixture_path()).unwrap()),
        "c137797755de759807a3e04f90fa349a01104072045e526ca22b353c25f72e9a"
    );
    let supports: Vec<_> = app
        .core
        .model
        .nodes
        .iter()
        .filter(|node| node.restraint != Dof6Mask::FREE)
        .collect();
    assert_eq!(supports.len(), 12);
    assert!(supports
        .iter()
        .all(|node| node.restraint == Dof6Mask::PINNED));
    assert_eq!(app.core.model.wall_assignment_regions.regions.len(), 62);
    for (index, region) in app
        .core
        .model
        .wall_assignment_regions
        .regions
        .iter_mut()
        .enumerate()
    {
        assert_eq!(region.id.0, index as u32);
        assert!(region.assignment.is_unset());
        region.assignment = sepika_core::model::PlateAssignment::NoPlate;
    }
    assert_eq!(
        app.core.model.unset_plate_assignment_regions(),
        (vec![], vec![])
    );
    select_frame_gravity(&mut app.core.model);
    let index = app.core.model.ultimate_gravity_combination.unwrap();
    app.core.model.combinations[index].name = "保存長期架構G+P".into();
    app
}

fn target(app: &App) -> ElemId {
    let ids: Vec<_> = [13, 14]
        .into_iter()
        .map(|source| {
            app.core
                .model
                .stb_node_ids
                .iter()
                .find(|n| n.id == source)
                .unwrap()
                .node
        })
        .collect();
    let matches: Vec<_> = app
        .core
        .model
        .elements
        .iter()
        .filter(|e| e.nodes.as_slice() == ids)
        .collect();
    assert_eq!(matches.len(), 1);
    matches[0].id
}

fn section(app: &App, name: &str, floor: Option<&str>) -> SectionId {
    let matches: Vec<_> = app
        .core
        .model
        .sections
        .iter()
        .filter(|s| s.name == name && s.floor.as_deref() == floor)
        .collect();
    assert_eq!(matches.len(), 1);
    assert!(sepika_edit::valid_study_section(
        &app.core.model,
        matches[0]
    ));
    matches[0].id
}

fn wait(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    while app.studies.running.is_some() {
        app.poll_study_calculation();
        assert!(
            std::time::Instant::now() < deadline,
            "代表モデルの計算未完了"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn representative_manual_study_retains_three_completed_runs() {
    let mut app = fixture();
    let target = target(&app);
    let candidates = [section(&app, "GY2", Some("2")), section(&app, "b6", None)];
    let model = app.core.model.clone();
    let path = std::env::var_os("SEPIKA_ISSUE985_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| test_tmp().join("issue985-manual.ovika"));
    app.save_project_to(path.with_file_name("issue985-input.ovika"));
    assert_no_error(&app, "計算前の専用入力保存");
    let s = app
        .create_study("Issue985 G43の既存断面比較".into(), vec![target])
        .unwrap();
    for (name, section) in [("A GY2", candidates[0]), ("B b6", candidates[1])] {
        let p = app.create_comparison(s, name.into()).unwrap();
        app.switch_study(Location::Proposal(s, p)).unwrap();
        assert!(app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(sepika_edit::SetElementSection {
                elem: target,
                section: Some(section)
            })
        ));
        app.core.scoped.staleness.mark_edited();
        app.switch_study(Location::Main).unwrap();
    }
    assert!(app.core.model.eq_ignoring_dofmap(&model));
    let mut observations = Vec::new();
    for p in 0..3 {
        app.start_study_calculation(s, p).unwrap();
        wait(&mut app);
        let run = app.studies.items[s].runs.last().unwrap();
        assert!(
            run.error.is_none(),
            "{}: {:?}",
            run.proposal_name,
            run.error
        );
        assert!(run.preparation.is_some());
        assert!(!run.comparison_checks.is_empty());
        let results = run.results.as_ref().unwrap();
        let combo = results
            .combos
            .iter()
            .find(|(name, _)| name == "保存長期架構G+P")
            .unwrap();
        let node = run
            .prepared
            .model
            .stb_node_ids
            .iter()
            .find(|n| n.id == 14)
            .unwrap()
            .node;
        let z = combo.1.disp[node.index()][2];
        assert!(z.is_finite() && z < 0.0);
        let checks = run
            .comparison_checks
            .iter()
            .find(|e| {
                serde_json::to_value(&e.bundle).unwrap()["design_source"]
                    == serde_json::json!({"Combo":"保存長期架構G+P"})
            })
            .unwrap();
        let target_checks = checks
            .bundle
            .member_checks
            .iter()
            .find(|m| m.elem == target)
            .unwrap();
        assert_eq!(target_checks.positions.len(), 3);
        assert!(target_checks.positions.iter().all(|position| matches!(
            position.outcome,
            sepika_design_jp::CheckOutcome::Checked(_)
        )));
        let whole_quantity_error = sepika_design_jp::quantity::try_compute_quantity_takeoff(
            &run.prepared.model,
            &run.prepared.model.quantity_cfg,
        )
        .unwrap_err();
        assert!(whole_quantity_error.contains("基礎梁の型枠対象側面"));
        let quantity = sepika_design_jp::quantity::steel_members::try_compute_steel_members(
            &run.prepared.model,
            &[target],
        )
        .unwrap();
        let steel = &quantity[0].steel;
        assert_eq!(steel.length_m, 4.0);
        let section = run
            .prepared
            .model
            .section(run.prepared.model.elements[target.index()].section.unwrap())
            .unwrap();
        let Some(sepika_core::section_shape::SectionShape::SteelH {
            height,
            width,
            web_thick,
            flange_thick,
            root_r: Some(root_r),
        }) = section.shape
        else {
            panic!("固定した圧延H形断面")
        };
        let area_mm2 = 2.0 * width * flange_thick
            + (height - 2.0 * flange_thick) * web_thick
            + (4.0 - std::f64::consts::PI) * root_r.powi(2);
        let volume_m3 = steel.weight_t / sepika_core::units::STEEL_UNIT_WEIGHT_TAKEOFF_T_M3;
        assert!((volume_m3 - area_mm2 * 4000.0 / 1e9).abs() < 1e-10);
        observations.push(serde_json::json!({"proposal":run.proposal_name,"input":run.input.references,"prepared":run.prepared.references,"target":target,"node14_z_mm":z,"steel_volume_m3":volume_m3,"whole_quantity_error":whole_quantity_error,"section":section,"target_checks":target_checks,"whole_member_checks":checks.bundle.member_checks.len(),"notice":run.notice,"error":run.error}));
        eprintln!(
            "Issue985 {} node14 Z={z:.12} mm, target={} section={:?}, checks={}",
            run.proposal_name,
            target.0,
            run.input.model.elements[target.index()].section,
            checks.bundle.member_checks.len()
        );
    }
    assert!(app.core.model.eq_ignoring_dofmap(&model));
    std::fs::write(
        path.with_extension("observed.json"),
        serde_json::to_vec_pretty(&observations).unwrap(),
    )
    .unwrap();
    let confirmation = app
        .confirm_study_run(s, 1, &app.studies.items[s].runs[1])
        .unwrap();
    let retained = rmp_serde::to_vec_named(&confirmation.run).unwrap();
    app.decide_study(
        s,
        confirmation,
        true,
        "既存検定の未評価範囲を残してGY2への変更内容を確認".into(),
    )
    .unwrap();
    assert_eq!(
        app.core.model.elements[target.index()].section,
        Some(candidates[0])
    );
    assert!(app.core.scoped.results.is_none());
    app.core.scoped.undo.undo(&mut app.core.model);
    assert!(app.core.model.eq_ignoring_dofmap(&model));
    assert!(!app.studies.items[s].decisions[0].is_applied());
    app.core.scoped.undo.redo(&mut app.core.model);
    assert!(app.studies.items[s].decisions[0].is_applied());
    assert!(app.core.scoped.undo.run(
        &mut app.core.model,
        Box::new(sepika_edit::SetElementSection {
            elem: target,
            section: Some(candidates[1])
        })
    ));
    app.core.scoped.undo.undo(&mut app.core.model);
    assert_eq!(
        app.core.model.elements[target.index()].section,
        Some(candidates[0])
    );
    app.core.scoped.undo.undo(&mut app.core.model);
    assert!(app.core.model.eq_ignoring_dofmap(&model));
    app.core.scoped.undo.redo(&mut app.core.model);
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_eq!(
        app.core.model.elements[target.index()].section,
        Some(candidates[1])
    );
    app.switch_study(Location::Proposal(s, 1)).unwrap();
    assert!(app.core.scoped.undo.run(
        &mut app.core.model,
        Box::new(sepika_edit::SetElementSection {
            elem: target,
            section: Some(candidates[1])
        })
    ));
    app.switch_study(Location::Main).unwrap();
    assert_eq!(
        rmp_serde::to_vec_named(&app.studies.items[s].decisions[0].confirmation.run).unwrap(),
        retained
    );
    app.save_project_to(path.clone());
    assert_no_error(&app, "代表モデル検討の保存");
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    assert_no_error(&loaded, "代表モデル検討の再読込");
    assert_eq!(loaded.studies.items[s].runs.len(), 3);
    assert_eq!(
        rmp_serde::to_vec_named(&loaded.studies.items[s].decisions[0].confirmation.run).unwrap(),
        retained
    );
    eprintln!("Issue985 保存先={}", path.display());
}

#[test]
fn issue985_representative_candidates_resolve_without_replacement() {
    let mut app = imported();
    assert_eq!(app.core.model.elements.len(), 115);
    let target = app
        .core
        .model
        .elements
        .iter()
        .find(|e| {
            e.nodes.len() == 2
                && app.core.model.nodes[e.nodes[0].index()].coord == [1000.0, 1000.0, 4700.0]
                && app.core.model.nodes[e.nodes[1].index()].coord == [5000.0, 1000.0, 4700.0]
        })
        .unwrap()
        .id;
    let element = &app.core.model.elements[target.index()];
    let endpoints: Vec<_> = element
        .nodes
        .iter()
        .map(|id| app.core.model.nodes[id.index()].coord)
        .collect();
    assert_eq!(
        endpoints,
        [[1000.0, 1000.0, 4700.0], [5000.0, 1000.0, 4700.0]]
    );
    let original = element.section.unwrap();
    assert_eq!(app.core.model.sections[original.index()].name, "GY1");
    for (name, floor, depth) in [("GY2", Some("2"), 700.0), ("b6", None, 450.0)] {
        let candidates: Vec<_> = app
            .core
            .model
            .sections
            .iter()
            .filter(|s| s.name == name && s.floor.as_deref() == floor)
            .collect();
        assert_eq!(
            candidates.len(),
            1,
            "候補断面 {name}/{floor:?} を一意に参照する"
        );
        let section = candidates[0];
        assert_eq!(section.depth, depth);
        assert!(
            sepika_edit::valid_study_section(&app.core.model, section),
            "候補{name}が検討に割当可能な鉄骨大梁断面である: {section:?}"
        );
        let id = section.id;
        assert!(
            app.core.scoped.undo.run(
                &mut app.core.model,
                Box::new(sepika_edit::SetElementSection {
                    elem: target,
                    section: Some(id)
                })
            ),
            "候補{name}が既存の有効断面として割当可能である"
        );
        assert_eq!(app.core.model.elements[target.index()].section, Some(id));
    }
}
