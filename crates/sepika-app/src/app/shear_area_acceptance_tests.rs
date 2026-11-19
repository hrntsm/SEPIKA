use super::*;
use sepika_core::model::{FrameSectionUse, NodalLoad, PropertyBasis, Section};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::CheckOutcome;

#[path = "../../../sepika-design-jp/tests/fixtures/steel_h_column_combined_model.rs"]
mod h_column_input;

#[cfg(feature = "gui")]
pub(super) fn design_table_contains(shape: &egui::epaint::Shape, needle: &str) -> bool {
    match shape {
        egui::epaint::Shape::Text(text) => text.galley.text().contains(needle),
        egui::epaint::Shape::Vec(parts) => {
            parts.iter().any(|part| design_table_contains(part, needle))
        }
        _ => false,
    }
}

fn cantilever(section: Section, load_n: [f64; 6]) -> App {
    let mut model = crate::sample::portal_frame();
    model.panel_zone = sepika_core::model::PanelZoneMode::None;
    model.stories.clear();
    model.nodes.truncate(2);
    model.nodes[0].coord = [0.0, 0.0, 0.0];
    model.nodes[1].coord = [1000.0, 0.0, 0.0];
    model.nodes[1].restraint = sepika_core::dof::Dof6Mask::FREE;
    let mut beam = model.elements[2].clone();
    beam.id = ElemId(0);
    beam.nodes = [NodeId(0), NodeId(1)].into_iter().collect();
    beam.section = Some(SectionId(0));
    model.elements = vec![beam];
    model.sections = vec![section];
    model.sections[0].frame_use = Some(FrameSectionUse::Girder);
    model.sections[0].material = Some(MaterialId(0));
    model.materials[0].density = 0.0;
    model.load_cases.truncate(1);
    model.load_cases[0].member.clear();
    model.load_cases[0].nodal = vec![NodalLoad::manual(NodeId(1), load_n)];
    let mut app = App::default();
    assert!(app.load_model(model));
    app.core.analysis_cfg.threads = 1;
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(app.require_selected_static_input().is_ok());
    app
}

pub(crate) fn unsupported_box() -> App {
    const SUPPLIED_J_MM4: f64 = 400_000_000.0;
    let mut section = SectionShape::SteelBox {
        height: 400.0,
        width: 300.0,
        thick: 12.0,
        horizontal_thick: 12.0,
        corner_r: Some(5.0),
    }
    .input_section(SectionId(0), "BOX適用外".into())
    .unwrap();
    section.j = SUPPLIED_J_MM4;
    section.property_basis.j = PropertyBasis::Supplied;
    cantilever(section, [0.0, 0.0, -100_000.0, 0.0, 0.0, 0.0])
}

#[test]
fn constructed_cantilever_matches_independent_bending_plus_shear_in_both_directions() {
    let cases = [
        (
            SectionShape::SteelH {
                height: 400.0,
                width: 200.0,
                web_thick: 8.0,
                flange_thick: 13.0,
                root_r: Some(0.0),
            },
            [229648682.66666666, 17349290.666666668],
            [3200.0, 5200.0],
        ),
        (
            SectionShape::SteelBox {
                height: 400.0,
                width: 300.0,
                thick: 12.0,
                horizontal_thick: 12.0,
                corner_r: Some(0.0),
            },
            [377380352.0, 241229952.0],
            [9024.0, 6624.0],
        ),
        (
            SectionShape::SteelBox {
                height: 300.0,
                width: 400.0,
                thick: 12.0,
                horizontal_thick: 12.0,
                corner_r: Some(0.0),
            },
            [241229952.0, 377380352.0],
            [6624.0, 9024.0],
        ),
    ];
    for (shape, inertia_mm4, area_mm2) in cases {
        for (global_direction, i_mm4, as_mm2) in [
            (2, inertia_mm4[0], area_mm2[0]),
            (1, inertia_mm4[1], area_mm2[1]),
        ] {
            let section = shape.to_section(SectionId(0), "独立片持ち".into());
            let mut load_n = [0.0; 6];
            load_n[global_direction] = 100_000.0;
            let app = cantilever(section, load_n);
            let delta_mm = app.current_static().unwrap().disp[1][global_direction];
            let expected_mm = 100_000.0 * 1000.0_f64.powi(3) / (3.0 * 205_000.0 * i_mm4)
                + 100_000.0 * 1000.0 / ((205_000.0 / 2.6) * as_mm2);
            assert!(
                (delta_mm / expected_mm - 1.0).abs() < 1e-8,
                "{delta_mm} / {expected_mm}"
            );
            let forces = &app.current_static().unwrap().member_forces[0].1.at[0].1;
            let (active, inactive) = if global_direction == 2 {
                (1, 2)
            } else {
                (2, 1)
            };
            assert!((forces[active].abs() / 100_000.0 - 1.0).abs() < 1e-10);
            assert!(forces[inactive].abs() < 1e-8);
        }
    }
}

#[test]
fn constructed_cantilever_keeps_supplied_stiffness_area() {
    let mut section = SectionShape::SteelH {
        height: 400.0,
        width: 200.0,
        web_thick: 8.0,
        flange_thick: 13.0,
        root_r: Some(0.0),
    }
    .to_section(SectionId(0), "供給剛性".into());
    section.as_z = 1600.0;
    section.property_basis.as_z = PropertyBasis::Supplied;
    let app = cantilever(section, [0.0, 0.0, 100_000.0, 0.0, 0.0, 0.0]);
    let delta_mm = app.current_static().unwrap().disp[1][2];
    let expected_mm = 100_000.0 * 1000.0_f64.powi(3) / (3.0 * 205_000.0 * 229648682.66666666)
        + 100_000.0 * 1000.0 / ((205_000.0 / 2.6) * 1600.0);
    assert!((delta_mm / expected_mm - 1.0).abs() < 1e-8);
    assert_eq!(app.core.model.sections[0].as_z, 1600.0);
}

fn assert_unchecked(app: &App) {
    let checks = &app.current_design_results().unwrap().member_checks;
    assert!(!checks.is_empty());
    assert!(checks.iter().flat_map(|m| &m.positions).all(|p| matches!(
        &p.outcome, CheckOutcome::Skipped { reason } if reason.contains("適用範囲外")
    )));
    let csv = crate::summary::build_report_csv(app);
    assert!(
        csv.contains(",-,検定不能,") && csv.contains("外角半径"),
        "{csv}"
    );
    assert!(!csv.contains(",OK,"), "{csv}");
}

#[test]
fn unsupported_box_keeps_input_and_skipped_outcomes_through_recheck_save_and_report() {
    let mut app = unsupported_box();
    assert_unchecked(&app);
    let input = rmp_serde::to_vec_named(&app.core.model).unwrap();
    app.run_design_check();
    assert_unchecked(&app);
    assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), input);
    let path =
        std::env::temp_dir().join(format!("sepika-417-skipped-{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    assert!(app.core.scoped.last_error.is_none());
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert!(reopened.core.scoped.last_error.is_none());
    assert_eq!(
        rmp_serde::to_vec_named(&reopened.core.model).unwrap(),
        input
    );
    assert_unchecked(&reopened);
    reopened.run_design_check();
    assert_unchecked(&reopened);
    assert_eq!(
        rmp_serde::to_vec_named(&reopened.core.model).unwrap(),
        input
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn flat_bar_shear_reason_reaches_report_gui_and_saved_project() {
    use sepika_core::result_contract::{ANALYSIS_CONTRACT_VERSION, DESIGN_CONTRACT_VERSION};
    use sepika_design_jp::{CheckComponent, CheckKind, CheckResult};

    let section = SectionShape::SteelFlatBar {
        width: 100.0,
        thick: 10.0,
    }
    .to_section(SectionId(0), "平鋼100×10".into());
    let mut app = cantilever(section, [0.0, 0.0, 100_000.0, 0.0, 0.0, 0.0]);
    let assert_unchecked = |app: &App| {
        let results = app.current_design_results().unwrap();
        assert_eq!(
            results.design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
        assert_eq!(results.member_checks.len(), 1);
        let member = &results.member_checks[0];
        assert_eq!(member.elem, ElemId(0));
        assert_eq!(member.positions.len(), 3);
        let csv = crate::summary::build_report_csv(app);
        for position in &member.positions {
            let CheckOutcome::Skipped { reason } = &position.outcome else {
                panic!("{:?}", position.outcome);
            };
            assert!(reason.contains("平鋼のせん断応力度"), "{reason}");
            assert!(reason.contains("許容値の組合せが未確定"), "{reason}");
            assert!(
                csv.lines()
                    .any(|line| line == format!("0,{:.3},-,検定不能,{reason}", position.xi)),
                "{csv}"
            );
        }
        assert!(!csv.contains(",OK,"), "{csv}");
        rmp_serde::to_vec_named(&results.member_checks).unwrap()
    };
    let expected = assert_unchecked(&app);
    let path =
        std::env::temp_dir().join(format!("sepika-1396-flat-bar-{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    assert!(app.core.scoped.last_error.is_none());
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert!(reopened.core.scoped.last_error.is_none());
    std::fs::remove_file(path).unwrap();
    assert_eq!(assert_unchecked(&reopened), expected);
    reopened.run_design_check();
    assert_eq!(assert_unchecked(&reopened), expected);
    #[cfg(feature = "gui")]
    {
        let context = egui::Context::default();
        let rendered = context.run_ui(Default::default(), |ui| {
            crate::design_view::design_table(ui, &mut reopened)
        });
        for needle in ["検定済み 0 件、検定不能 3 件", "平鋼のせん断応力度"] {
            assert!(
                rendered
                    .shapes
                    .iter()
                    .any(|shape| design_table_contains(&shape.shape, needle)),
                "{needle}"
            );
        }
    }
    let legacy_basis = "旧版の平鋼せん断検定";
    app.core.scoped.results.as_mut().unwrap().member_checks[0].positions[0].outcome =
        CheckOutcome::Checked(CheckResult {
            basis: legacy_basis.into(),
            detail: String::new(),
            components: vec![CheckComponent {
                kind: CheckKind::Shear,
                ratio: 0.884_451_476_205_384,
                detail: String::new(),
            }],
        });
    assert!(crate::summary::build_report_csv(&app).contains(&format!(",OK,{legacy_basis}")));
    for version in [57, 58] {
        let (mut old, path) = super::result_contract_tests::restore(
            &app,
            Some(ANALYSIS_CONTRACT_VERSION),
            Some(version),
            &format!("flat-bar-shear-1396-design{version}"),
        );
        std::fs::remove_file(path).unwrap();
        assert!(old.current_static().is_some());
        assert!(old.current_design_results().is_none());
        assert!(!crate::summary::build_report_csv(&old).contains(legacy_basis));
        old.run_design_check();
        assert_eq!(assert_unchecked(&old), expected);
    }
}

#[test]
fn h_column_combination_reason_reaches_design_table_report_and_saved_project() {
    check_h_column_combination_report(77_000.0, 167e6, 365_000.0, "1454");
    check_h_column_combination_report(-77_000.0, 50e6, 2_000.0, "1483");
}

fn check_h_column_combination_report(
    axial_n: f64,
    strong_moment_nmm: f64,
    weak_shear_n: f64,
    issue: &str,
) {
    use sepika_core::result_contract::{ANALYSIS_CONTRACT_VERSION, DESIGN_CONTRACT_VERSION};
    use sepika_design_jp::{CheckComponent, CheckKind, CheckResult};

    let model = h_column_input::cantilever(axial_n, strong_moment_nmm, weak_shear_n);
    let mut app = App::default();
    assert!(app.load_model(model));
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );

    let assert_unchecked = |app: &App| {
        let results = app.current_design_results().unwrap();
        assert_eq!(
            results.design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
        let [member] = results.member_checks.as_slice() else {
            panic!("{:?}", results.member_checks);
        };
        assert_eq!(member.elem, ElemId(0));
        assert_eq!(member.positions.len(), 3);
        let csv = crate::summary::build_report_csv(app);
        for position in &member.positions {
            let CheckOutcome::Skipped { reason } = &position.outcome else {
                panic!("{:?}", position.outcome);
            };
            assert!(
                reason.contains("フランジ内の設計用公称応力度の分布を規準から確定できず"),
                "{reason}"
            );
            assert!(
                csv.lines()
                    .any(|line| line == format!("0,{:.3},-,検定不能,{reason}", position.xi)),
                "{csv}"
            );
        }
        assert!(!csv.contains(",OK,"), "{csv}");
        rmp_serde::to_vec_named(&results.member_checks).unwrap()
    };
    let expected = assert_unchecked(&app);
    let path = std::env::temp_dir().join(format!(
        "sepika-{issue}-h-column-{}.ovika",
        std::process::id()
    ));
    app.save_project_to(path.clone());
    assert!(app.core.scoped.last_error.is_none());
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert!(reopened.core.scoped.last_error.is_none());
    std::fs::remove_file(path).unwrap();
    assert_eq!(assert_unchecked(&reopened), expected);

    #[cfg(feature = "gui")]
    {
        let context = egui::Context::default();
        let rendered = context.run_ui(Default::default(), |ui| {
            crate::design_view::design_table(ui, &mut reopened)
        });
        for needle in [
            "検定済み 0 件、検定不能 3 件",
            "フランジ内の設計用公称応力度",
        ] {
            assert!(
                rendered
                    .shapes
                    .iter()
                    .any(|shape| design_table_contains(&shape.shape, needle)),
                "{needle}"
            );
        }
    }

    let legacy_basis = "旧版のH形鋼柱の合成応力検定";
    app.core.scoped.results.as_mut().unwrap().member_checks[0].positions[0].outcome =
        CheckOutcome::Checked(CheckResult {
            basis: legacy_basis.into(),
            detail: String::new(),
            components: vec![CheckComponent {
                kind: CheckKind::Shear,
                ratio: 0.992_447,
                detail: String::new(),
            }],
        });
    assert!(crate::summary::build_report_csv(&app).contains(&format!(",OK,{legacy_basis}")));
    let (mut old, path) = super::result_contract_tests::restore(
        &app,
        Some(ANALYSIS_CONTRACT_VERSION),
        Some(DESIGN_CONTRACT_VERSION - 1),
        &format!("h-column-combination-{issue}-old"),
    );
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        old.current_static().unwrap().member_forces,
        app.current_static().unwrap().member_forces
    );
    assert!(old.current_design_results().is_none());
    assert!(!crate::summary::build_report_csv(&old).contains(legacy_basis));
    old.run_design_check();
    assert_eq!(assert_unchecked(&old), expected);
}

#[test]
fn h_column_combination_report_keeps_confirmed_axial_bending_ng_and_reason() {
    let mut app = App::default();
    assert!(app.load_model(h_column_input::cantilever(-77_000.0, 500e6, 365_000.0)));
    app.run_linear_static(LoadCaseId(0));
    let results = app.current_design_results().unwrap();
    let csv = crate::summary::build_report_csv(&app);
    for position in &results.member_checks[0].positions {
        let CheckOutcome::Checked(result) = &position.outcome else {
            panic!("{:?}", position.outcome);
        };
        assert!(!result.ok(), "{result:?}");
        assert_eq!(result.components.len(), 1);
        assert_eq!(
            result.components[0].kind,
            sepika_design_jp::CheckKind::AxialBending
        );
        assert!(
            csv.lines()
                .any(|line| line.starts_with(&format!("0,{:.3},", position.xi))
                    && line.contains(",NG,")
                    && line.contains("同一点の組合せ応力度は未算定")),
            "{csv}"
        );
    }
}
