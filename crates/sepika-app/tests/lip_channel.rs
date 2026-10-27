use sepika_app::{app::App, sample, summary::build_report_csv};
use sepika_core::{ids::SectionId, model::Model, section_shape::SectionShape};
use sepika_design_jp::{CheckOutcome, DesignCheck, DesignCtx, MemberForcesAt, SteelDesign};

fn lip_model() -> Model {
    let mut model = sample::portal_frame();
    model.panel_zone = sepika_core::model::PanelZoneMode::None;
    for sec in &mut model.sections {
        let shape = SectionShape::SteelLipChannel {
            height: 150.0,
            width: 75.0,
            lip: 20.0,
            thick: 2.3,
        };
        let mut lip = shape.to_section(sec.id, sec.name.clone());
        lip.material = sec.material;
        lip.frame_use = sec.frame_use;
        *sec = lip;
    }
    model
}

fn reason(model: &Model) -> String {
    let mut sec = model.sections[0].clone();
    sec.name = "往復断面".into();
    let forces = MemberForcesAt {
        pos: 0.5,
        n: -100_000.0,
        qy: 1000.0,
        qz: -2000.0,
        my: -1_000_000.0,
        mz: 2_000_000.0,
    };
    match SteelDesign.check(&forces, &sec, &model.materials[0], &DesignCtx::default()) {
        CheckOutcome::Skipped { reason } => reason,
        CheckOutcome::Checked(_) => panic!("リップ材は未検定"),
    }
}

#[test]
fn lip_channel_internal_edit_and_ovika_preserve_unsupported_reason_and_gross_properties() {
    let model = lip_model();
    model.validate().unwrap();
    let path = std::env::temp_dir().join(format!("sepika509_{}.ovika", std::process::id()));
    sepika_io::ovika::save_ovika(
        &path,
        &model,
        sepika_io::ovika::OvikaExtras {
            preparation: None,
            results: None,
            analysis_settings: None,
        },
    )
    .unwrap();
    let saved = sepika_io::ovika::load_ovika(&path).unwrap().model;
    let xml = sepika_io::stbridge::export_stbridge(&model).unwrap();
    assert!(xml.contains("<StbSecRoll-LipC "));
    let mut edited = sample::portal_frame();
    let mut undo = sepika_edit::UndoStack::new();
    for section in &model.sections {
        assert!(undo.run(
            &mut edited,
            Box::new(sepika_edit::EditSectionShape {
                section: section.id,
                new_shape: section.shape.clone().unwrap(),
                frame_use: section.frame_use,
            })
        ));
    }
    for back in [&saved, &edited] {
        back.validate().unwrap();
        let a = &model.sections[0];
        let b = &back.sections[0];
        assert_eq!(a.shape, b.shape);
        for (old, new) in [(a.area, b.area), (a.iy, b.iy), (a.iz, b.iz), (a.j, b.j)] {
            assert!(
                (old - new).abs() <= old.abs().max(1.0) * 1e-12,
                "{old} != {new}"
            );
        }
        assert_eq!(reason(&model), reason(back));
    }
}

#[test]
fn lip_channel_actual_member_checks_and_report_remain_unchecked() {
    let mut app = App::default();
    app.load_model(lip_model());
    app.run_preparation();
    app.run_static_all();
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let checks = &app.core.scoped.results.as_ref().unwrap().member_checks;
    assert!(!checks.is_empty());
    for member in checks {
        assert!(!member.positions.is_empty());
        for position in &member.positions {
            let CheckOutcome::Skipped { reason } = &position.outcome else {
                panic!("リップ材の検定位置がChecked")
            };
            for item in [
                "[局部座屈]",
                "[ゆがみ座屈]",
                "[全体座屈]",
                "有効A[mm²]・有効Z[mm³]は未算定",
            ] {
                assert!(reason.contains(item));
            }
        }
    }
    let csv = build_report_csv(&app);
    let rows = csv
        .split("[部材検定]\n")
        .nth(1)
        .unwrap()
        .split("\n[")
        .next()
        .unwrap();
    assert!(rows.contains(",-,検定不能,リップ溝形鋼:"), "{rows}");
    assert!(!rows.contains(",OK,"));
    assert!(!rows.contains(",NG,"));
    assert!(rows.contains("[局部座屈]"));
    assert!(rows.contains("[ゆがみ座屈]"));
    assert!(rows.contains("[全体座屈]"));
    let path = std::env::temp_dir().join(format!("sepika509_results_{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut restored = App::default();
    restored.open_project_from(path);
    assert!(
        restored.core.scoped.last_error.is_none(),
        "{:?}",
        restored.core.scoped.last_error
    );
    assert!(build_report_csv(&restored).contains(rows));
    restored.run_design_check();
    assert!(
        restored.core.scoped.last_error.is_none(),
        "{:?}",
        restored.core.scoped.last_error
    );
    assert!(build_report_csv(&restored).contains(rows));
}

#[test]
fn lip_channel_invalid_geometry_is_rejected_by_existing_mass_input() {
    use sepika_core::model::SectionMassProperties;
    let model = lip_model();
    for (height, width, lip, thick) in [
        (150.0, 75.0, 20.0, 0.0),
        (150.0, 75.0, 20.0, -1.0),
        (150.0, 75.0, 20.0, f64::NAN),
        (f64::INFINITY, 75.0, 20.0, 2.3),
        (150.0, 2.0, 20.0, 2.3),
        (150.0, 75.0, 1.0, 2.3),
        (150.0, 75.0, 149.0, 2.3),
    ] {
        let mut sec = model.sections[0].clone();
        sec.name = "不正リップ".into();
        sec.shape = Some(SectionShape::SteelLipChannel {
            height,
            width,
            lip,
            thick,
        });
        assert!(SectionMassProperties::try_from_section(
            &sec,
            Some(&model.materials[0]),
            None,
            None,
            None
        )
        .is_err());
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            my: 0.0,
            mz: 0.0,
        };
        let CheckOutcome::Skipped { reason } =
            SteelDesign.check(&forces, &sec, &model.materials[0], &DesignCtx::default())
        else {
            panic!("不正入力で検定済み")
        };
        assert!(reason.contains("寸法不正"), "{reason}");
    }
}

#[cfg(feature = "gui")]
#[test]
fn lip_channel_egui_design_table_displays_unchecked_reason() {
    let mut app = lip_app_with_direct_buckling_length(1000.0);
    let lambda = 1000.0_f64 / 28.08671071462023;
    assert!(lambda < 200.0);
    assert!((lambda - 35.60402676414013).abs() < 1e-12);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let results = app.core.scoped.results.as_mut().unwrap();
    results.member_checks.truncate(1);
    results.member_checks[0].positions.truncate(1);
    let text = rendered_design_table(&mut app);
    assert!(text.contains("検定済み 0 位置、検定不能 1 位置"), "{text}");
    assert!(text.contains("リップ溝形鋼"), "{text}");
    assert!(!text.contains("OK"), "{text}");
}

#[cfg(feature = "gui")]
fn lip_app_with_direct_buckling_length(length_mm: f64) -> App {
    let mut model = lip_model();
    let section = &model.sections[0];
    assert!((section.area - 760.84).abs() < 1e-10);
    assert!((section.iy - 2804950.3045333335).abs() < 1e-8);
    assert!((section.iz - 600198.7674505038).abs() < 1e-8);
    model.steel_design_attrs = model
        .elements
        .iter()
        .map(|elem| sepika_core::model::SteelDesignAttr {
            elem: elem.id,
            joint_flange_loss: 0.0,
            joint_web_loss: 0.0,
            scallop_web_loss: 0.0,
            lb_direct: None,
            lateral_brace_count: None,
            lk_y_direct: Some(length_mm),
            lk_z_direct: Some(length_mm),
            c_direct: None,
        })
        .collect();
    let mut app = App::default();
    app.load_model(model);
    app.run_linear_static(sepika_core::ids::LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app
}

#[cfg(feature = "gui")]
#[test]
fn lip_channel_egui_reports_statutory_failure_before_unsupported_stress_check() {
    let mut app = lip_app_with_direct_buckling_length(6000.0);
    let results = app.core.scoped.results.as_mut().unwrap();
    results.member_checks.truncate(1);
    results.member_checks[0].positions.truncate(1);
    let CheckOutcome::Checked(result) = &results.member_checks[0].positions[0].outcome else {
        panic!("確定した法定上限超過を未検定へ隠さない")
    };
    assert!(!result.ok());
    assert_eq!(result.components.len(), 1);
    let component = &result.components[0];
    assert_eq!(component.kind, sepika_design_jp::CheckKind::Provision);
    assert!((component.ratio - 1.068120802924204).abs() < 1e-12);
    assert!(
        component.detail.contains("法定上限200")
            && component.detail.contains("応力度比ではありません")
    );
    let text = rendered_design_table(&mut app);
    assert!(
        text.contains("検定済み 1 位置、検定不能 0 位置、NG 1 件"),
        "{text}"
    );
    assert!(
        text.contains("有効細長比上限") && text.contains("構造規定"),
        "{text}"
    );
    assert!(!text.contains("OK"), "{text}");
}

#[cfg(feature = "gui")]
fn rendered_design_table(app: &mut App) -> String {
    let ctx = egui::Context::default();
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(2400.0, 1400.0),
            )),
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                sepika_app::design_view::design_table(ui, app);
            });
        },
    );
    fn texts(shape: &egui::epaint::Shape, out: &mut String) {
        match shape {
            egui::epaint::Shape::Text(text) => out.push_str(text.galley.text()),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    texts(shape, out);
                }
            }
            _ => {}
        }
    }
    let mut text = String::new();
    for shape in &output.shapes {
        texts(&shape.shape, &mut text);
    }
    text
}

#[cfg(feature = "gui")]
#[test]
fn mixed_egui_design_table_counts_only_checked_positions_as_checked() {
    let mut model = lip_model();
    model.sections[0] = sample::portal_frame().sections[0].clone();
    let mut app = App::default();
    app.load_model(model);
    app.run_linear_static(sepika_core::ids::LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let checks = &app.core.scoped.results.as_ref().unwrap().member_checks;
    let checked = checks
        .iter()
        .flat_map(|m| &m.positions)
        .filter(|p| matches!(p.outcome, CheckOutcome::Checked(_)))
        .count();
    let skipped = checks
        .iter()
        .flat_map(|m| &m.positions)
        .filter(|p| matches!(p.outcome, CheckOutcome::Skipped { .. }))
        .count();
    assert!(checked > 0 && skipped > 0);
    let text = rendered_design_table(&mut app);
    assert!(
        text.contains(&format!(
            "検定済み {checked} 位置、検定不能 {skipped} 位置、NG 0 件"
        )),
        "{text}"
    );
    assert!(text.contains("OK"), "{text}");
    assert!(text.contains("検定不能"), "{text}");
}

#[test]
fn lip_channel_geometry_shared_mass_and_beam_fiber_ms_reject_without_fallback() {
    use sepika_core::model::{AnalysisKind, ElementKind, SectionMassProperties};
    use sepika_element::frame::{
        beam::BeamElement, fiber::FiberBeam, multi_spring::MultiSpringElement,
    };
    let mut model = lip_model();
    let material = model.element_material(&model.elements[0]).unwrap();
    let section = &model.sections[0];
    let mass =
        SectionMassProperties::try_from_section(section, Some(material), None, None, None).unwrap();
    assert!((mass.mass_per_length - 5.972594e-6).abs() < 1e-15);
    assert!((mass.total_mass(3000.0) - 0.017917782).abs() < 1e-12);
    for lip in [75.0, 80.0, 100.0] {
        model.sections[0].shape = Some(SectionShape::SteelLipChannel {
            height: 150.0,
            width: 75.0,
            lip,
            thick: 2.3,
        });
        model.sections[0].property_basis = Default::default();
        for kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
        ] {
            let mut element = model
                .elements
                .iter()
                .find(|e| e.section == Some(SectionId(0)))
                .unwrap()
                .clone();
            element.kind = kind;
            assert!(model
                .element_mass_properties(&element)
                .unwrap_err()
                .contains("H=150"));
            assert!(BeamElement::try_new(&element, &model)
                .err()
                .unwrap()
                .contains("H=150"));
            assert!(FiberBeam::try_new(
                &element,
                &model,
                sepika_element::factory::StrengthBasis::default(),
                AnalysisKind::Incremental
            )
            .err()
            .unwrap()
            .contains("H=150"));
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                MultiSpringElement::new(
                    &element,
                    &model,
                    sepika_element::factory::StrengthBasis::default(),
                    AnalysisKind::Incremental,
                )
            }))
            .is_err());
        }
    }
}
