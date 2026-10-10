use sepika_app::app::{App, ResultsBundle, StaticKey};
use sepika_core::ids::{LoadCaseId, MaterialId, NodeId, SectionId, WallPlateId, WallRegionId};
use sepika_core::model::{MaterialCategory, WallPlate, WallPlateShape, WallRegion};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::wall_check::{WallCheckKind, WallCheckSummary, WallSkipKind};
use sepika_element::frame::beam::MemberForces;

fn fixture() -> App {
    let mut app = App::default();
    let model = &mut app.core.model;
    *model = sepika_app::sample::portal_frame();
    let mut concrete = model.materials[0].clone();
    concrete.id = MaterialId(1);
    concrete.name = "Fc24".into();
    concrete.category = MaterialCategory::Concrete;
    concrete.fc = Some(24.0);
    concrete.fy = None;
    model.materials.push(concrete);
    let mut rebar = model.materials[0].clone();
    rebar.id = MaterialId(2);
    rebar.name = "SD345".into();
    rebar.category = MaterialCategory::Rebar;
    rebar.fy = Some(345.0);
    model.materials.push(rebar);
    let mut rc = SectionShape::RcWall {
        thickness: 180.0,
        ps: 0.006,
        pwh_ratio: Some(0.006),
    }
    .to_section(SectionId(2), "RC壁".into());
    rc.material = Some(MaterialId(1));
    rc.rebar_material = Some(MaterialId(2));
    rc.shear_rebar_material = Some(MaterialId(2));
    model.sections.push(rc.clone());
    let mut steel = rc.clone();
    steel.id = SectionId(3);
    steel.shape = None;
    steel.material = Some(MaterialId(0));
    steel.thickness = Some(6.0);
    model.sections.push(steel);
    rc.id = SectionId(4);
    rc.material = None;
    model.sections.push(rc);
    for (index, section) in [2, 3, 4].into_iter().enumerate() {
        let boundary = if index == 0 {
            [NodeId(0), NodeId(1), NodeId(3), NodeId(2)]
        } else {
            let lower = NodeId(model.nodes.len() as u32);
            let upper = NodeId(lower.0 + 1);
            let y = if index == 1 { 6000.0 } else { -6000.0 };
            let mut bottom = model.nodes[1].clone();
            bottom.id = lower;
            bottom.coord = [0.0, y, 0.0];
            let mut top = model.nodes[3].clone();
            top.id = upper;
            top.coord = [0.0, y, 3500.0];
            model.nodes.extend([bottom, top]);
            let first_id = model.elements.iter().map(|e| e.id.0).max().unwrap() + 1;
            for (offset, source, nodes) in [(0, 1, [lower, upper]), (1, 2, [NodeId(2), upper])] {
                let mut member = model.elements[source].clone();
                member.id = sepika_core::ids::ElemId(first_id + offset);
                member.nodes = nodes.into_iter().collect();
                model.elements.push(member);
            }
            [NodeId(0), lower, upper, NodeId(2)]
        };
        model.add_enclosed_wall_plate_from_nodes(
            &boundary,
            WallPlate {
                id: WallPlateId(0),
                dl_support: None,
                self_weight_shares: vec![],
                shape: WallPlateShape::Enclosed,
                section: Some(SectionId(section)),
                opening_area: 0.0,
                opening_weight: 0.0,
                openings: vec![],
                loads: vec![],
                slit: Default::default(),
            },
        );
        model.wall_regions.push(WallRegion {
            id: WallRegionId(index as u32),
            name: format!("共通側柱壁{index}"),
            boundary: boundary.to_vec(),
            wall_plate_ids: vec![WallPlateId(index as u32)],
            posts: vec![],
        });
    }
    sepika_job::prepare::apply_rigid_zones_and_panels(model);
    let (expanded, _, _) = sepika_load::wall_expand::expand_wall_elements(model);
    let forces = expanded
        .elements
        .iter()
        .filter(|e| e.kind == sepika_core::model::ElementKind::Wall)
        .map(|e| {
            (
                e.id,
                MemberForces {
                    at: vec![(0.0, [0.0; 6])],
                },
            )
        })
        .collect();
    // 明示した無載荷Pを含む保存G+P+Wで、壁の検定状態を検証する。
    let mut live = model.load_cases[0].clone();
    live.id = LoadCaseId(2);
    live.kind = sepika_core::model::LoadCaseKind::Live;
    live.nodal.clear();
    live.member.clear();
    model.load_cases.push(live);
    model.load_cases[1].kind = sepika_core::model::LoadCaseKind::Wind;
    model
        .combinations
        .push(sepika_core::model::LoadCombination {
            name: "保存G+P+W".into(),
            terms: vec![
                (LoadCaseId(0), 1.0),
                (LoadCaseId(2), 1.0),
                (LoadCaseId(1), 1.0),
            ],
        });
    let mut results = ResultsBundle::default();
    results.member_forces = forces;
    results.combos.push((
        "保存G+P+W".into(),
        sepika_solver::statics::linear::StaticOnce {
            disp: vec![],
            member_forces: results.member_forces.clone(),
            panel_moments: vec![],
        },
    ));
    app.core.scoped.results = Some(results);
    app.core.scoped.last_static = Some(StaticKey::Combo(0));
    app.core.design_term = sepika_design_jp::LoadTerm::Short;
    app.run_design_check();
    app
}

#[test]
fn wall_status_app_csv_save_and_recalculation_keep_keys_and_none() {
    let mut app = fixture();
    let checks = &app.core.scoped.results.as_ref().unwrap().wall_checks;
    assert_eq!(checks.len(), 6);
    assert!(checks.iter().all(|w| w.case == "combo:0:保存G+P+W"));
    let s = WallCheckSummary::for_kind(checks, WallCheckKind::AllowableShear);
    assert_eq!((s.n_walls, s.n_ok, s.n_skipped), (3, 1, 2), "{checks:?}");
    assert_eq!(s.max_ratio, Some(0.0));
    let csv = sepika_app::summary::build_report_csv(&app);
    assert!(
        csv.contains("[耐震壁種別集計]\n種別,壁枚数,合格,NG,未検定,対象外\n許容せん断,3,1,0,2,0"),
        "{csv}"
    );
    assert!(csv.contains("国内許容応力度・終局検定式は未確定"));
    assert!(csv.contains(",\"combo:0:保存G+P+W\",許容せん断,,未検定,MissingInput"));
    let expected_checks = format!("{checks:?}");
    let path = std::env::temp_dir().join(format!(
        "sepika495-wall-status-{}.ovika",
        std::process::id()
    ));
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let saved = sepika_io::ovika::load_ovika(&path).unwrap();
    let restored: sepika_app::app::SavedResults =
        rmp_serde::from_slice(saved.results.as_ref().unwrap()).unwrap();
    assert_eq!(restored.bundle.wall_checks.len(), 6);
    assert_eq!(
        format!("{:?}", restored.bundle.wall_checks),
        expected_checks
    );
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert!(
        reopened.core.scoped.last_error.is_none(),
        "{:?}",
        reopened.core.scoped.last_error
    );
    assert_eq!(
        format!(
            "{:?}",
            reopened.core.scoped.results.as_ref().unwrap().wall_checks
        ),
        expected_checks
    );
    let forces = app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .member_forces
        .clone();
    let bundle = app.core.scoped.results.as_mut().unwrap();
    bundle.combos.push((
        "DL+E,独立".into(),
        sepika_solver::statics::linear::StaticOnce {
            disp: vec![],
            member_forces: forces,
            panel_moments: vec![],
        },
    ));
    app.core
        .model
        .combinations
        .push(sepika_core::model::LoadCombination {
            name: "DL+E,独立".into(),
            terms: vec![
                (LoadCaseId(0), 1.0),
                (LoadCaseId(2), 1.0),
                (LoadCaseId(1), 1.0),
            ],
        });
    app.select_displayed_result(StaticKey::Combo(1));
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .wall_checks
        .iter()
        .all(|w| w.case == "combo:1:DL+E,独立"));
    let csv = sepika_app::summary::build_report_csv(&app);
    assert!(csv.contains("\"combo:1:DL+E,独立\""));
    let identity: Vec<_> = app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .wall_checks
        .iter()
        .map(|w| (w.plate, w.elem, w.kind))
        .collect();
    let other = app.core.scoped.results.as_ref().unwrap().combos[1]
        .1
        .clone();
    app.core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .combos
        .push(("DL-E".into(), other));
    app.core
        .model
        .combinations
        .push(sepika_core::model::LoadCombination {
            name: "DL-E".into(),
            terms: vec![
                (LoadCaseId(0), 1.0),
                (LoadCaseId(2), 1.0),
                (LoadCaseId(1), -1.0),
            ],
        });
    app.select_displayed_result(StaticKey::Combo(2));
    let checks = &app.core.scoped.results.as_ref().unwrap().wall_checks;
    assert!(checks.iter().all(|w| w.case == "combo:2:DL-E"));
    assert_eq!(
        checks
            .iter()
            .map(|w| (w.plate, w.elem, w.kind))
            .collect::<Vec<_>>(),
        identity
    );
    let bundle = app.core.scoped.results.as_mut().unwrap();
    bundle.member_forces.clear();
    app.run_design_check();
    let checks = &app.core.scoped.results.as_ref().unwrap().wall_checks;
    assert_eq!(WallCheckSummary::from_checks(checks).max_ratio, None);
    assert!(checks
        .iter()
        .filter(|w| w.plate == Some(WallPlateId(0)))
        .all(|w| w.skip_kind == Some(WallSkipKind::MissingResponse)));
    app.core.scoped.staleness.mark_edited();
    assert!(app.core.scoped.staleness.design_stale);
    let csv = sepika_app::summary::build_report_csv(&app);
    assert!(csv.contains("3,6,0,0,6,0,\n"));
    assert!(csv.contains("要再計算,モデル編集前の検定結果です"));
    app.core.model.load_cases.clear();
    app.run_static_all();
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .is_none_or(|r| r.wall_checks.is_empty()));
}

#[cfg(feature = "gui")]
#[test]
fn wall_status_real_egui_design_table_shows_counts_and_reasons() {
    let mut app = fixture();
    let context = egui::Context::default();
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(4000.0, 4000.0),
            )),
            ..Default::default()
        },
        |ui| sepika_app::design_view::design_table(ui, &mut app),
    );
    let text = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.job.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    for expected in [
        "許容せん断：合格 1・NG 0・未検定 2",
        "壁版 1",
        "壁版 2",
        "combo:0:保存G+P+W",
        "国内許容応力度・終局検定式は未確定",
        "壁主材料が未割当",
    ] {
        assert!(text.contains(expected), "{expected}: {text}");
    }
}
