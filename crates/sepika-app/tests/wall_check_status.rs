use sepika_app::app::{App, StaticKey};
use sepika_core::ids::{LoadCaseId, MaterialId, NodeId, SectionId, WallPlateId, WallRegionId};
use sepika_core::model::{MaterialCategory, WallPlate, WallPlateShape, WallRegion};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::wall_check::{WallCheckKind, WallCheckSummary, WallSkipKind};

fn fixture() -> App {
    let mut app = App::default();
    let model = &mut app.core.model;
    *model = sepika_app::sample::portal_frame();
    model.panel_zone = sepika_core::model::PanelZoneMode::None;
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
    rc.shear_rebar_material = None;
    model.sections.push(rc);
    for (id, original) in [(5, 0), (6, 1)] {
        let mut section = model.sections[original].clone();
        section.id = SectionId(id);
        model.sections.push(section);
    }
    for (id, shape) in [
        (
            0,
            SectionShape::RcColumnRect {
                b: 300.0,
                d: 300.0,
                rebar: sepika_core::section_shape::RcRectColumnRebar {
                    main_dia: 22.0,
                    x: vec![4],
                    y: vec![4],
                    cover: 40.0,
                    hoop: sepika_core::section_shape::RectColumnHoop {
                        dia: 10.0,
                        pitch: 100.0,
                        legs_x: 2,
                        legs_y: 2,
                    },
                },
            },
        ),
        (
            1,
            SectionShape::RcBeamRect {
                b: 300.0,
                d: 400.0,
                rebar: sepika_core::section_shape::RcBeamRebar {
                    main_dia: 22.0,
                    top: vec![2],
                    bottom: vec![2],
                    cover: 40.0,
                    stirrup: sepika_core::section_shape::BeamStirrup {
                        dia: 10.0,
                        pitch: 100.0,
                        legs: 2,
                    },
                },
            },
        ),
    ] {
        let mut section = shape.to_section(SectionId(id as u32), format!("RC支持材{id}"));
        section.material = Some(MaterialId(1));
        section.rebar_material = Some(MaterialId(2));
        section.shear_rebar_material = Some(MaterialId(2));
        section.frame_use = model.sections[id].frame_use;
        model.sections[id] = section;
    }
    for (index, section) in [2, 3, 4].into_iter().enumerate() {
        let boundary = if index == 0 {
            [NodeId(0), NodeId(1), NodeId(3), NodeId(2)]
        } else if index == 1 {
            let first = model.nodes.len() as u32;
            for (offset, coord) in [
                [0.0, 6000.0, 0.0],
                [6000.0, 6000.0, 0.0],
                [6000.0, 6000.0, 3500.0],
                [0.0, 6000.0, 3500.0],
            ]
            .into_iter()
            .enumerate()
            {
                let mut node = model.nodes[if offset < 2 { 0 } else { 2 }].clone();
                node.id = NodeId(first + offset as u32);
                node.coord = coord;
                model.nodes.push(node);
            }
            [
                NodeId(first),
                NodeId(first + 1),
                NodeId(first + 2),
                NodeId(first + 3),
            ]
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
                dl_support: Some(sepika_core::model::WallDlSupport::LowerBeam),
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
        for element in &mut model.elements {
            if element.section.is_none() {
                let vertical = model.nodes[element.nodes[0].index()].coord[2]
                    != model.nodes[element.nodes[1].index()].coord[2];
                element.section = Some(SectionId(if index == 1 {
                    if vertical {
                        5
                    } else {
                        6
                    }
                } else if vertical {
                    0
                } else {
                    1
                }));
            }
        }
        model.wall_regions.push(WallRegion {
            id: WallRegionId(index as u32),
            name: format!("共通側柱壁{index}"),
            boundary: boundary.to_vec(),
            wall_plate_ids: vec![WallPlateId(index as u32)],
            posts: vec![],
        });
    }
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
    for case in &mut model.load_cases {
        case.nodal.clear();
        case.member.clear();
    }
    model.load_cases[1].name = "固定支点への水平力W".into();
    model.load_cases[1]
        .nodal
        .push(sepika_core::model::NodalLoad::manual(
            NodeId(0),
            [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ));
    app.run_combination(0);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let results = app.core.scoped.results.as_ref().unwrap();
    assert_eq!(
        results.analysis_contract_version,
        Some(sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION)
    );
    assert_eq!(app.core.scoped.last_static, Some(StaticKey::Combo(0)));
    assert!(!results.member_forces.is_empty());
    assert!(results.member_forces.iter().all(|(_, forces)| forces
        .at
        .iter()
        .all(|(_, values)| values.iter().all(|value| *value == 0.0))));
    assert!(!results.combos[0].1.disp.is_empty());
    assert!(results.combos[0]
        .1
        .disp
        .iter()
        .flatten()
        .all(|value| *value == 0.0));
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
    app.run_combination(1);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
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
    app.run_combination(2);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
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
    app.core.model.sections[0].area *= 1.1;
    app.run_design_check();
    assert!(app
        .core
        .scoped
        .last_error
        .as_ref()
        .unwrap()
        .contains("一致しません"));
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .wall_checks
        .is_empty());
    assert!(!sepika_app::summary::build_report_csv(&app).contains("許容せん断,3,1,0,2,0"));
    app.run_combination(2);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let checks = &app.core.scoped.results.as_ref().unwrap().wall_checks;
    assert_eq!(checks.len(), 6);
    assert_eq!(
        checks
            .iter()
            .map(|w| (w.plate, w.elem, w.kind))
            .collect::<Vec<_>>(),
        identity
    );
    assert_eq!(
        WallCheckSummary::for_kind(checks, WallCheckKind::AllowableShear).max_ratio,
        Some(0.0)
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
    let csv = sepika_app::summary::build_report_csv(&app);
    assert!(csv.contains("3,6,0,0,6,0,\n"));
    app.core.model.nodes[0].coord[0] += 1.0;
    app.core.scoped.staleness.mark_edited();
    assert!(app.core.scoped.staleness.design_stale);
    let csv = sepika_app::summary::build_report_csv(&app);
    assert!(csv.contains("[検定結果状態]\n利用不可"));
    assert!(!csv.contains("[耐震壁検定集計]"));
    app.core.model.nodes[0].coord[0] -= 1.0;
    app.core.model.elements[0].section = Some(SectionId(5));
    app.run_combination(2);
    assert!(app
        .core
        .scoped
        .last_error
        .as_ref()
        .unwrap()
        .contains("混合構造は扱えません"));
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .wall_checks
        .is_empty());
    app.core.model.elements[0].section = Some(SectionId(0));
    app.core.model.materials[1].fc = None;
    app.run_combination(2);
    assert!(app
        .core
        .scoped
        .last_error
        .as_ref()
        .unwrap()
        .contains("Fcが未設定"));
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .wall_checks
        .is_empty());
    app.core.model.materials[1].fc = Some(24.0);
    app.core.model.sections[4].material = None;
    app.run_combination(2);
    assert!(app
        .core
        .scoped
        .last_error
        .as_ref()
        .unwrap()
        .contains("主材料が未設定"));
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .wall_checks
        .is_empty());
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
        "横筋材料が未割当",
    ] {
        assert!(text.contains(expected), "{expected}: {text}");
    }
}
