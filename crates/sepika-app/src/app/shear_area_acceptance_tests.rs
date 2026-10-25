use super::*;
use sepika_core::model::{FrameSectionUse, NodalLoad, PropertyBasis, Section};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::CheckOutcome;

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
