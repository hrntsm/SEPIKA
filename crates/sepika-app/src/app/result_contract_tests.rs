use super::*;
use sepika_core::result_contract::{ANALYSIS_CONTRACT_VERSION, DESIGN_CONTRACT_VERSION};

fn calculated() -> App {
    let mut app = App::default();
    let mut model = crate::sample::portal_frame();
    model.panel_zone = sepika_core::model::PanelZoneMode::None;
    app.load_model(crate::app::tests::with_explicit_pushover_gravity(model));
    app.core.analysis_cfg.threads = 1;
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(!app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .member_checks
        .is_empty());
    assert!(!app
        .core
        .model
        .elements
        .iter()
        .any(|element| matches!(element.kind, sepika_core::model::ElementKind::PanelZone)));
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .panel_moments
        .is_empty());
    app
}

struct ContractFixture<'a>(&'a SavedResults);
impl serde::Serialize for ContractFixture<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        struct Bundle<'a>(&'a ResultsBundle);
        impl serde::Serialize for Bundle<'_> {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("holding_evaluations", &self.0.holding_evaluations)?;
                map.serialize_entry("statics", &self.0.statics)?;
                map.serialize_entry("combos", &self.0.combos)?;
                map.serialize_entry("modal", &self.0.modal)?;
                map.serialize_entry("member_forces", &self.0.member_forces)?;
                map.serialize_entry("panel_moments", &self.0.panel_moments)?;
                map.serialize_entry("member_checks", &self.0.member_checks)?;
                map.serialize_entry("joint_checks", &self.0.joint_checks)?;
                map.serialize_entry("wall_checks", &self.0.wall_checks)?;
                map.serialize_entry("beam_checks", &self.0.beam_checks)?;
                map.serialize_entry("slab_checks", &self.0.slab_checks)?;
                map.serialize_entry("pushover", &self.0.pushover)?;
                map.serialize_entry("pushover_x", &self.0.pushover_x)?;
                map.serialize_entry("pushover_y", &self.0.pushover_y)?;
                map.serialize_entry("time_history", &self.0.time_history)?;
                map.serialize_entry("time_histories", &self.0.time_histories)?;
                map.serialize_entry("lumped", &self.0.lumped)?;
                map.serialize_entry("lumped_results", &self.0.lumped_results)?;
                map.serialize_entry("input_records", &self.0.input_records)?;
                map.serialize_entry("design_source", &self.0.design_source)?;
                if let Some(version) = self.0.analysis_contract_version {
                    map.serialize_entry("analysis_contract_version", &version)?;
                }
                if let Some(version) = self.0.design_contract_version {
                    map.serialize_entry("design_contract_version", &version)?;
                }
                map.end()
            }
        }
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("bundle", &Bundle(&self.0.bundle))?;
        map.serialize_entry("last_static", &self.0.last_static)?;
        map.serialize_entry("last_run", &self.0.last_run)?;
        map.end()
    }
}

pub(super) fn restore(
    app: &App,
    analysis: Option<u32>,
    design: Option<u32>,
    name: &str,
) -> (App, std::path::PathBuf) {
    let mut bundle = app.core.scoped.results.clone().unwrap();
    bundle.analysis_contract_version = analysis;
    bundle.design_contract_version = design;
    let saved = SavedResults {
        bundle,
        last_static: app.core.scoped.last_static,
        view_vibration_case: None,
        view_lumped_vibration_case: None,
        last_run: app.core.scoped.staleness.last_run,
    };
    let results = rmp_serde::to_vec_named(&ContractFixture(&saved)).unwrap();
    let settings = rmp_serde::to_vec_named(&SavedAnalysisSettings {
        cfg: app.core.analysis_cfg,
        wave_name: None,
        wave_sha256: None,
        lumped_wave_name: None,
        lumped_wave_sha256: None,
    })
    .unwrap();
    let path = std::env::temp_dir().join(format!("sepika-587-{}-{name}.ovika", std::process::id()));
    sepika_io::ovika::save_ovika(
        &path,
        &app.core.model,
        sepika_io::ovika::OvikaExtras {
            results: Some(&results),
            analysis_settings: Some(&settings),
            ..Default::default()
        },
    )
    .unwrap();
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
    (loaded, path)
}

#[test]
fn ovika_missing_old_exact_future_contracts_preserve_model_and_only_exact_results() {
    let app = calculated();
    for (name, version, reason) in [
        ("missing", None, "版不明"),
        ("old", Some(0), "旧契約"),
        ("old-section-mass", Some(2), "旧契約"),
        ("old-src-overlap", Some(3), "旧契約"),
        ("old-static-settings", Some(4), "旧契約"),
        ("old-zero-increment", Some(5), "旧契約"),
        ("old-auto-solver", Some(6), "旧契約"),
        ("old-steel-grade-recognition", Some(7), "旧契約"),
        ("old-frame-support-representation", Some(8), "旧契約"),
        ("old-signed-reactions", Some(9), "旧契約"),
        ("old-skew-rectangle", Some(10), "旧契約"),
        ("old-fixed-distance-extents", Some(11), "旧契約"),
        ("old-frame-node-counts", Some(12), "旧契約"),
        ("old-time-history-timeline", Some(19), "旧契約"),
        ("old-column-self-weight-faces", Some(20), "旧契約"),
        ("old-member-load-direction", Some(22), "旧契約"),
        ("old-gravity-run-provenance", Some(23), "旧契約"),
        ("old-attached-primary-support-loads", Some(24), "旧契約"),
        ("old-before-wall-side-column-contract", Some(26), "旧契約"),
        ("old-unconverged-time-history", Some(27), "旧契約"),
        ("old-panel-reference-column", Some(28), "旧契約"),
        ("exact", Some(ANALYSIS_CONTRACT_VERSION), ""),
        ("future", Some(ANALYSIS_CONTRACT_VERSION + 1), "未来版"),
    ] {
        let (loaded, path) = restore(&app, version, Some(DESIGN_CONTRACT_VERSION), name);
        assert_eq!(
            rmp_serde::to_vec_named(&loaded.core.model).unwrap(),
            rmp_serde::to_vec_named(&app.core.model).unwrap()
        );
        assert_eq!(loaded.core.analysis_cfg.threads, 1);
        let bundle = loaded.core.scoped.results.as_ref().unwrap();
        if reason.is_empty() {
            assert_eq!(
                bundle.statics[0].1.disp,
                app.core.scoped.results.as_ref().unwrap().statics[0].1.disp
            );
            assert_eq!(loaded.core.scoped.last_static, app.core.scoped.last_static);
            assert!(!bundle.member_checks.is_empty());
            assert!(!loaded.core.scoped.staleness.results_stale);
            assert!(loaded.require_selected_static_input().is_ok());
            assert!(loaded.current_design_results().is_some());
            assert_eq!(
                crate::summary::build_report_csv(&loaded),
                crate::summary::build_report_csv(&app)
            );
        } else {
            assert!(!bundle.has_analysis_results());
            assert!(!bundle.has_design_results());
            assert!(loaded.core.scoped.last_static.is_none());
            assert!(loaded.core.scoped.staleness.results_stale);
            let notice = loaded.core.scoped.result_contract_notice.as_ref().unwrap();
            assert!(
                notice.contains(reason) && notice.contains("保存版") && notice.contains("現行版")
            );
            let csv = crate::summary::build_report_csv(&loaded);
            assert!(csv.contains(reason));
            assert!(!csv.contains("全適合"));
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn ovika_old_analysis_is_unavailable_until_explicit_reanalysis() {
    let app = calculated();
    for version in [5, 6, 7, 20, 22, 24, 26, 28] {
        let (mut loaded, path) = restore(
            &app,
            Some(version),
            Some(DESIGN_CONTRACT_VERSION),
            &format!("old-analysis-{version}-recovery"),
        );
        let old = loaded.core.scoped.results.as_ref().unwrap();
        assert!(!old.has_analysis_results() && !old.has_design_results());
        assert!(loaded.core.scoped.staleness.results_stale);
        loaded.select_displayed_result(app.core.scoped.last_static.unwrap());
        loaded.core.scoped.staleness.mark_fresh();
        assert!(loaded.require_selected_static_input().is_err());
        assert!(!crate::summary::build_report_csv(&loaded).contains("全適合"));
        loaded.save_project_to(path.clone());
        assert!(loaded.core.scoped.last_error.is_none());
        let mut reopened = App::default();
        reopened.open_project_from(path.clone());
        assert!(reopened.core.scoped.last_error.is_none());
        let unavailable = reopened.core.scoped.results.as_ref().unwrap();
        assert!(!unavailable.has_analysis_results() && !unavailable.has_design_results());
        assert_ne!(
            unavailable.analysis_contract_version,
            Some(ANALYSIS_CONTRACT_VERSION)
        );
        assert!(reopened.require_selected_static_input().is_err());
        loaded = reopened;
        loaded.run_design_check();
        let design_only = loaded.core.scoped.results.as_ref().unwrap();
        assert!(!design_only.has_analysis_results());
        assert!(design_only.member_forces.is_empty() && design_only.member_checks.is_empty());
        assert!(loaded.require_selected_static_input().is_err());
        loaded.run_linear_static(LoadCaseId(0));
        assert!(loaded.core.scoped.last_error.is_none());
        let recovered = loaded.core.scoped.results.as_ref().unwrap();
        assert!(recovered.has_analysis_results() && recovered.has_design_results());
        assert_eq!(
            recovered.analysis_contract_version,
            Some(ANALYSIS_CONTRACT_VERSION)
        );
        assert_eq!(
            recovered.design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
        assert_eq!(
            recovered.statics[0].1.disp,
            app.core.scoped.results.as_ref().unwrap().statics[0].1.disp
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn ovika_old_design_keeps_forces_until_explicit_recheck_without_display_save_or_mark_fresh_restamping(
) {
    let mut app = App::default();
    app.load_model(super::tests::lateral_bracing_fixture::frame(true));
    app.run_linear_static(LoadCaseId(0));
    assert!(app.core.scoped.last_error.is_none());
    let checks = &mut app.core.scoped.results.as_mut().unwrap().member_checks;
    let old_check = checks
        .iter_mut()
        .flat_map(|member| &mut member.positions)
        .find_map(|p| match &mut p.outcome {
            sepika_design_jp::CheckOutcome::Checked(check)
                if check.detail.contains("（参考）n=2") =>
            {
                Some(check)
            }
            _ => None,
        })
        .expect("保存する横補剛参考detail");
    old_check.basis = "旧横補剛の合否".into();
    assert!(crate::summary::build_report_csv(&app).contains("旧横補剛の合否"));
    let mut in_memory = App::default();
    in_memory.load_model(app.core.model.clone());
    in_memory.core.scoped.results = app.core.scoped.results.clone();
    in_memory
        .core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .design_contract_version = Some(0);
    in_memory.core.scoped.ds_beta_u_by_story = vec![0.5];
    assert!(in_memory.reject_result_contracts());
    assert_eq!(in_memory.core.scoped.ds_beta_u_by_story, vec![0.5]);
    assert_eq!(
        in_memory
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .member_forces,
        app.core.scoped.results.as_ref().unwrap().member_forces
    );
    for (name, version) in [
        ("design-missing", None),
        ("design-old", Some(0)),
        ("design-old-signed-reactions", Some(3)),
        ("design-old-skew-rectangle", Some(4)),
        ("design-old-rc-q0", Some(11)),
        ("design-old-secondary-load-ownership", Some(16)),
        ("design-old-before-wall-side-column-contract", Some(18)),
        ("design-old-panel-reference-column", Some(19)),
        ("design-future", Some(DESIGN_CONTRACT_VERSION + 1)),
    ] {
        let (mut loaded, path) = restore(&app, Some(ANALYSIS_CONTRACT_VERSION), version, name);
        assert_eq!(
            rmp_serde::to_vec_named(&loaded.core.model).unwrap(),
            rmp_serde::to_vec_named(&app.core.model).unwrap()
        );
        let forces = loaded
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .member_forces
            .clone();
        assert_eq!(
            forces,
            app.core.scoped.results.as_ref().unwrap().member_forces
        );
        assert!(!loaded.core.scoped.staleness.results_stale);
        assert!(loaded.core.scoped.staleness.design_stale);
        assert!(
            loaded
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .member_checks
                .is_empty(),
            "旧横補剛detail/合否を除外"
        );
        let csv = crate::summary::build_report_csv(&loaded);
        assert!(!csv.contains("旧横補剛の合否") && !csv.contains("全適合"));
        loaded.select_displayed_result(app.core.scoped.last_static.unwrap());
        loaded.core.scoped.staleness.mark_fresh();
        loaded.save_project_to(path.clone());
        let mut reopened = App::default();
        reopened.open_project_from(path.clone());
        let bundle = reopened.core.scoped.results.as_ref().unwrap();
        assert_eq!(bundle.member_forces, forces);
        assert!(bundle.member_checks.is_empty());
        assert_eq!(bundle.design_contract_version, None);
        reopened.run_design_check();
        assert!(
            reopened.core.scoped.last_error.is_none(),
            "{:?}",
            reopened.core.scoped.last_error
        );
        let bundle = reopened.core.scoped.results.as_ref().unwrap();
        assert!(!bundle.member_checks.is_empty());
        assert!(reopened.core.scoped.result_contract_notice.is_none());
        assert_eq!(
            bundle.design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn selected_case_and_saved_combo_name_require_matching_input_but_ignore_display_axes() {
    for combo in [false, true] {
        for missing in [false, true] {
            let mut app = calculated();
            let combo_index = app.core.model.combinations.len();
            if combo {
                app.core
                    .model
                    .combinations
                    .push(sepika_core::model::LoadCombination {
                        name: "照合対象".into(),
                        terms: vec![(LoadCaseId(0), 1.0)],
                    });
                app.run_combination(combo_index);
                let Some(StaticKey::Combo(result_index)) = app.core.scoped.last_static else {
                    panic!("保存組合せの解析結果が選択されていません");
                };
                assert_eq!(
                    app.core.scoped.results.as_ref().unwrap().combos[result_index].0,
                    "照合対象"
                );
            }
            if missing {
                app.core
                    .scoped
                    .results
                    .as_mut()
                    .unwrap()
                    .input_records
                    .clear();
            } else {
                app.core.model.sections[0].area *= 1.1;
            }
            app.run_design_check();
            let reason = app.core.scoped.last_error.as_ref().unwrap();
            assert!(
                reason.contains(if missing {
                    "入力識別情報がありません"
                } else {
                    "一致しません"
                }),
                "{reason}"
            );
            assert!(reason.contains(if combo {
                "照合対象"
            } else {
                "静的荷重ケース"
            }));
            assert!(app
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .member_checks
                .is_empty());
            if combo {
                app.run_combination(combo_index);
            } else {
                app.run_linear_static(LoadCaseId(0));
            }
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
            app.core.model.axes.push(sepika_core::model::AxisGroup {
                name: "表示".into(),
                kind: sepika_core::model::AxisGroupKind::Other,
                axes: Vec::new(),
            });
            app.run_design_check();
            assert!(!app
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .member_checks
                .is_empty());
        }
    }
}

#[test]
fn old_analysis_is_excluded_before_partial_success_or_failure_without_restamping_other_runs() {
    let mut app = calculated();
    app.run_eigen(1);
    app.core.analysis_cfg.push_steps = 2;
    app.generate_stories_action();
    app.run_seismic(SeismicDir::X);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app.run_pushover();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    super::tests::select_holding_points(&mut app);
    use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;
    app.adopt_holding_evaluation(EvaluationPurpose::Ds).unwrap();
    app.adopt_holding_evaluation(EvaluationPurpose::HoldingCapacity)
        .unwrap();
    assert!(!app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .holding_evaluations
        .is_empty());
    app.core.analysis_cfg.th_duration = 0.1;
    app.run_time_history_sample();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .time_history
        .is_some());
    assert!(app.core.scoped.results.as_ref().unwrap().modal.is_some());
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .pushover_x
        .is_some());
    let (mut loaded, path) = restore(
        &app,
        Some(0),
        Some(DESIGN_CONTRACT_VERSION),
        "all-analysis-old",
    );
    assert!(!loaded
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .has_analysis_results());
    #[cfg(feature = "gui")]
    assert!(loaded.ui.scoped.time_history_data.time.is_empty());
    assert!(loaded.core.scoped.stick_response.is_none());
    loaded.run_linear_static(LoadCaseId(0));
    let recovered = loaded.core.scoped.results.as_ref().unwrap();
    assert!(
        recovered.modal.is_none()
            && recovered.pushover_x.is_none()
            && recovered.time_histories.is_empty()
            && recovered.holding_evaluations.is_empty()
    );
    assert_eq!(recovered.statics.len(), 1);
    std::fs::remove_file(path).unwrap();
    app.core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .analysis_contract_version = Some(0);
    app.run_linear_static(LoadCaseId(0));
    let bundle = app.core.scoped.results.as_ref().unwrap();
    assert_eq!(
        bundle.analysis_contract_version,
        Some(ANALYSIS_CONTRACT_VERSION)
    );
    assert!(
        bundle.modal.is_none()
            && bundle.pushover.is_none()
            && bundle.pushover_x.is_none()
            && bundle.pushover_y.is_none()
    );
    assert!(
        bundle.time_history.is_none()
            && bundle.time_histories.is_empty()
            && bundle.holding_evaluations.is_empty()
    );
    #[cfg(feature = "gui")]
    assert!(app.ui.scoped.time_history_data.time.is_empty());
    assert_eq!(bundle.statics.len(), 1);
    app.core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .analysis_contract_version = Some(0);
    app.run_linear_static(LoadCaseId(999));
    let bundle = app.core.scoped.results.as_ref().unwrap();
    assert_eq!(bundle.analysis_contract_version, None);
    assert!(!bundle.has_analysis_results());
    assert!(app.core.scoped.last_error.is_some());
}

#[test]
fn empty_bundle_is_not_an_unknown_result_and_direct_floor_design_survives_stale_state() {
    let mut app = App::default();
    app.core.scoped.results = Some(ResultsBundle::default());
    assert!(!app.reject_result_contracts());
    assert!(app.core.scoped.last_notice.is_none());
    for stale in [false, true] {
        let mut model = super::tests::make_square_slab_test_model();
        let slab_section = SectionId(model.sections.len() as u32);
        model.sections.push(
            sepika_core::section_shape::SectionShape::RcSlab { thickness: 150.0 }
                .to_section(slab_section, "S15".into()),
        );
        let mut concrete = crate::sample::portal_frame().materials[0].clone();
        concrete.category = sepika_core::model::MaterialCategory::Concrete;
        concrete.fc = Some(24.0);
        concrete.density = 2.4e-9;
        model.sections[slab_section.0 as usize].material = Some(concrete.id);
        model.materials.push(concrete);
        model.slabs[0].plate.section = Some(slab_section);
        let mut app = App::default();
        app.load_model(model);
        app.core.scoped.staleness.results_stale = stale;
        app.run_design_check();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let bundle = app.core.scoped.results.as_ref().unwrap();
        assert_eq!(bundle.analysis_contract_version, None);
        assert_eq!(bundle.slab_checks.len(), 1);
        assert_eq!(
            bundle.design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
        let (mut loaded, path) = restore(
            &app,
            None,
            Some(0),
            if stale { "slab-stale" } else { "slab-fresh" },
        );
        assert!(loaded
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .slab_checks
            .is_empty());
        assert!(loaded
            .core
            .scoped
            .result_contract_notice
            .as_ref()
            .unwrap()
            .contains("旧契約"));
        loaded.run_design_check();
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "{:?}",
            loaded.core.scoped.last_error
        );
        assert_eq!(
            loaded
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .slab_checks
                .len(),
            1
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(feature = "gui")]
#[test]
fn gui_reports_old_contract_and_explicit_recheck_button_recovers_without_reanalysis() {
    fn text_position(shape: &egui::epaint::Shape, needle: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text().contains(needle) => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            egui::epaint::Shape::Vec(parts) => {
                parts.iter().find_map(|part| text_position(part, needle))
            }
            _ => None,
        }
    }
    let app = calculated();
    let (mut loaded, path) = restore(
        &app,
        Some(ANALYSIS_CONTRACT_VERSION),
        Some(0),
        "gui-recheck",
    );
    let forces = loaded
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .member_forces
        .clone();
    let context = egui::Context::default();
    let rendered = context.run_ui(Default::default(), |ui| {
        crate::design_view::design_table(ui, &mut loaded)
    });
    assert!(rendered
        .shapes
        .iter()
        .any(|s| text_position(&s.shape, "旧契約").is_some()));
    let button = rendered
        .shapes
        .iter()
        .find_map(|s| text_position(&s.shape, "検定を再実行"))
        .unwrap();
    assert_eq!(
        loaded
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .design_contract_version,
        None
    );
    for pressed in [true, false] {
        let input = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(button),
                egui::Event::PointerButton {
                    pos: button,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            ..Default::default()
        };
        let _ = context.run_ui(input, |ui| {
            crate::design_view::design_table(ui, &mut loaded)
        });
    }
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
    let recovered = loaded.core.scoped.results.as_ref().unwrap();
    assert_eq!(
        recovered.design_contract_version,
        Some(DESIGN_CONTRACT_VERSION)
    );
    assert!(!recovered.member_checks.is_empty());
    assert_eq!(recovered.member_forces, forces);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn ovika_current_selected_input_missing_or_mismatched_is_excluded_without_losing_other_runs() {
    for combo in [false, true] {
        for missing in [false, true] {
            let mut app = calculated();
            if combo {
                app.core
                    .model
                    .combinations
                    .push(sepika_core::model::LoadCombination {
                        name: "保存照合".into(),
                        terms: vec![(LoadCaseId(0), 1.0)],
                    });
                app.run_combination(0);
            }
            app.run_eigen(1);
            if missing {
                app.core
                    .scoped
                    .results
                    .as_mut()
                    .unwrap()
                    .input_records
                    .retain(|(key, _)| matches!(key, ResultInputKey::Modal));
            } else {
                app.core.model.sections[0].area *= 1.1;
            }
            let (mut loaded, path) = restore(
                &app,
                Some(ANALYSIS_CONTRACT_VERSION),
                Some(DESIGN_CONTRACT_VERSION),
                &format!("input-{combo}-{missing}"),
            );
            let bundle = loaded.core.scoped.results.as_ref().unwrap();
            assert!(bundle.member_checks.is_empty() && bundle.member_forces.is_empty());
            assert!(bundle.panel_moments.is_empty());
            assert!(
                bundle.modal.is_some(),
                "非選択の固有値成果まで一括除外しない"
            );
            assert!(loaded.core.scoped.last_static.is_none());
            assert!(
                loaded.core.scoped.staleness.results_stale
                    && loaded.core.scoped.staleness.design_stale
            );
            let expected = if missing {
                "入力識別情報"
            } else {
                "一致しません"
            };
            assert!(crate::summary::build_report_csv(&loaded).contains(expected));
            if combo {
                loaded.run_combination(0);
            } else {
                loaded.run_linear_static(LoadCaseId(0));
            }
            assert!(loaded.core.scoped.last_error.is_none());
            assert!(!loaded
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .member_checks
                .is_empty());
            std::fs::remove_file(path).unwrap();
        }
    }
    let mut invalid_selection = calculated();
    invalid_selection.core.scoped.last_static = Some(StaticKey::Combo(999));
    let (loaded, path) = restore(
        &invalid_selection,
        Some(ANALYSIS_CONTRACT_VERSION),
        Some(DESIGN_CONTRACT_VERSION),
        "missing-selected-combo",
    );
    let bundle = loaded.core.scoped.results.as_ref().unwrap();
    assert!(bundle.member_forces.is_empty() && bundle.panel_moments.is_empty());
    assert!(bundle.member_checks.is_empty());
    assert!(loaded.core.scoped.last_static.is_none());
    std::fs::remove_file(path).unwrap();
    let mut app = calculated();
    app.core.model.axes.push(sepika_core::model::AxisGroup {
        name: "表示だけの変更".into(),
        kind: sepika_core::model::AxisGroupKind::Other,
        axes: Vec::new(),
    });
    let (loaded, path) = restore(
        &app,
        Some(ANALYSIS_CONTRACT_VERSION),
        Some(DESIGN_CONTRACT_VERSION),
        "display-only-loaded",
    );
    assert!(!loaded.core.scoped.staleness.results_stale);
    assert!(!loaded
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .member_checks
        .is_empty());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn issue635_invalid_correct_hash_project_keeps_app_model_and_undo() {
    use sepika_core::model::{MemberLoad, MemberLoadKind};
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let mut app = calculated();
    let mut invalid = app.core.model.clone();
    let elem = invalid.elements[0].id;
    let length_mm = invalid.member_length(&invalid.elements[0]);
    invalid.load_cases[0].member.push(MemberLoad::manual(
        elem,
        [0.0, 0.0, -1.0],
        MemberLoadKind::Point {
            a: length_mm + 0.5,
            p: 1.0,
        },
    ));
    let bytes = rmp_serde::to_vec_named(&invalid).unwrap();
    let settings = b"{}";
    let hash = |b: &[u8]| format!("{:x}", Sha256::digest(b));
    let manifest = serde_json::json!({"schema_version":1,"units":"internal: N-mm-s","created_by":"independent-635-fixture","entries":[{"name":"model.msgpack","sha256":hash(&bytes)},{"name":"settings.json","sha256":hash(settings)}]});
    let path = std::env::temp_dir().join(format!(
        "sepika635-app-invalid-{}.ovika",
        std::process::id()
    ));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options = zip::write::FileOptions::<()>::default();
    for (name, data) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("model.msgpack", bytes),
        ("settings.json", settings.to_vec()),
    ] {
        zip.start_file(name, options).unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap();
    let before = format!("{:?}", app.core.model);
    let revision = app.core.scoped.undo.revision();
    let analysis_version = app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .analysis_contract_version;
    app.open_project_from(path);
    assert_eq!(format!("{:?}", app.core.model), before);
    assert_eq!(app.core.scoped.undo.revision(), revision);
    assert_eq!(
        app.core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .analysis_contract_version,
        analysis_version
    );
    let reason = app.core.scoped.last_error.as_ref().unwrap();
    assert!(
        reason.contains("member[") && reason.contains("部材") && reason.contains("mm"),
        "{reason}"
    );
}

#[test]
fn lip_channel_geometry_old_contracts_expire_and_current_keeps_results() {
    let app = calculated();
    for (version, keep) in [
        (13, false),
        (14, false),
        (15, false),
        (ANALYSIS_CONTRACT_VERSION, true),
    ] {
        let (loaded, path) = restore(
            &app,
            Some(version),
            Some(DESIGN_CONTRACT_VERSION),
            &format!("lip544-contract-{version}"),
        );
        let bundle = loaded.core.scoped.results.as_ref().unwrap();
        assert_eq!(bundle.has_analysis_results(), keep);
        if keep {
            assert_eq!(
                bundle.analysis_contract_version,
                Some(ANALYSIS_CONTRACT_VERSION)
            );
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn shear_modulus_gate_expires_analysis29_and_keeps_current_results() {
    let app = calculated();
    for (version, keep) in [(29, false), (ANALYSIS_CONTRACT_VERSION, true)] {
        let (loaded, path) = restore(
            &app,
            Some(version),
            Some(DESIGN_CONTRACT_VERSION),
            &format!("shear712-contract-{version}"),
        );
        let results = loaded.core.scoped.results.as_ref().unwrap();
        assert_eq!(results.has_analysis_results(), keep);
        assert_eq!(results.has_design_results(), keep);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn cft_case2_design12_expires_while_current_analysis_survives_and_design_can_be_recomputed() {
    let app = super::tests::cft_long_case2_app(4_000_000.0);
    let original = app.core.scoped.results.as_ref().unwrap();
    assert!(original.has_design_results());
    let (mut loaded, path) = restore(
        &app,
        Some(ANALYSIS_CONTRACT_VERSION),
        Some(12),
        "cft-case2-design12",
    );
    let bundle = loaded.core.scoped.results.as_ref().unwrap();
    assert_eq!(bundle.statics[0].1.disp, original.statics[0].1.disp);
    assert_eq!(bundle.member_forces, original.member_forces);
    assert_eq!(
        bundle.analysis_contract_version,
        Some(ANALYSIS_CONTRACT_VERSION)
    );
    assert!(!bundle.has_design_results());
    assert!(loaded.core.scoped.staleness.design_stale);
    assert!(!loaded.core.scoped.staleness.results_stale);
    loaded.select_displayed_result(app.core.scoped.last_static.unwrap());
    loaded.core.scoped.staleness.mark_fresh();
    loaded.save_project_to(path.clone());
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert!(reopened.core.scoped.last_error.is_none());
    let saved = reopened.core.scoped.results.as_ref().unwrap();
    assert_eq!(
        saved.analysis_contract_version,
        Some(ANALYSIS_CONTRACT_VERSION)
    );
    assert!(!saved.has_design_results());
    assert_eq!(saved.design_contract_version, None);
    reopened.run_design_check();
    assert!(
        reopened.core.scoped.last_error.is_none(),
        "{:?}",
        reopened.core.scoped.last_error
    );
    let recalculated = reopened.core.scoped.results.as_ref().unwrap();
    assert_eq!(
        recalculated.design_contract_version,
        Some(DESIGN_CONTRACT_VERSION)
    );
    assert!(recalculated.has_design_results());
    assert_eq!(recalculated.member_forces, original.member_forces);
    let rows = reopened.compute_cft_ultimate_checks().unwrap();
    assert!((rows[0].mu_nm.as_ref().unwrap() - 450_915_163.991_432_7).abs() < 0.1);
    std::fs::remove_file(path).unwrap();
}

fn sm520_app(thickness: f64, fy: Option<f64>) -> App {
    let mut model = crate::sample::portal_frame();
    model.panel_zone = sepika_core::model::PanelZoneMode::None;
    for mat in &mut model.materials {
        mat.name = "SM520B".into();
        mat.fy = fy;
    }
    for sec in &mut model.sections {
        let shape = sepika_core::section_shape::SectionShape::SteelH {
            height: 1000.0,
            width: 400.0,
            web_thick: 80.0,
            flange_thick: thickness,
            root_r: Some(0.0),
        };
        let mut replacement = shape.to_section(sec.id, sec.name.clone());
        replacement.frame_use = sec.frame_use;
        replacement.material = sec.material;
        *sec = replacement;
    }
    let mut app = App::default();
    app.load_model(model);
    app.core.analysis_cfg.threads = 1;
    app.run_preparation();
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app
}
#[test]
fn sm520_gui_recheck_save_read_report_and_holding_keep_out_of_range_reasons() {
    use sepika_design_jp::CheckOutcome;
    for fy in [None, Some(177.0), Some(235.0), Some(335.0), Some(999.0)] {
        let mut app = sm520_app(100.001, fy);
        for _ in 0..2 {
            app.run_design_check();
            let checks = &app.core.scoped.results.as_ref().unwrap().member_checks;
            assert!(!checks.is_empty());
            assert!(checks.iter().flat_map(|m| &m.positions).all(|p| matches!(&p.outcome, CheckOutcome::Skipped { reason } if reason.contains("適用範囲"))));
            let report = crate::summary::build_report_csv(&app);
            assert!(
                report.contains("適用範囲") && report.contains("検定不能"),
                "{report}"
            );
        }
        for auto in [false, true] {
            app.core.design_rank_auto = auto;
            assert!(app
                .compute_holding_capacity()
                .err()
                .unwrap()
                .contains("適用範囲"));
            assert!(app.core.scoped.ds_rank_fallback_stories.is_empty());
        }
        app.run_preparation();
        assert!(crate::summary::build_preparation_csv(&app).contains("適用範囲"));
        app.run_linear_static(LoadCaseId(0));
        let path = std::env::temp_dir().join(format!(
            "sepika-629-current-{}-{:?}.ovika",
            std::process::id(),
            fy
        ));
        app.save_project_to(path.clone());
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert!(crate::summary::build_report_csv(&loaded).contains("適用範囲"));
        loaded.select_displayed_result(StaticKey::Case(StaticCaseKey::User(LoadCaseId(0))));
        assert!(crate::summary::build_report_csv(&loaded).contains("適用範囲"));
        loaded.run_linear_static(LoadCaseId(0));
        assert!(loaded.core.scoped.results.as_ref().unwrap().member_checks.iter().flat_map(|m| &m.positions).all(|p| matches!(&p.outcome, CheckOutcome::Skipped { reason } if reason.contains("適用範囲"))));
        std::fs::remove_file(path).unwrap();
    }
}
#[test]
fn sm520_old_335_and_out_of_range_success_expire_without_save_or_display_reauthentication() {
    use sepika_design_jp::{CheckComponent, CheckKind, CheckOutcome, CheckResult};
    for t in [80.0, 100.001] {
        let mut app = sm520_app(t, Some(335.0));
        for position in app
            .core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .member_checks
            .iter_mut()
            .flat_map(|m| &mut m.positions)
        {
            position.outcome = CheckOutcome::Checked(CheckResult {
                basis: "旧SM520成功335".into(),
                detail: String::new(),
                components: vec![CheckComponent {
                    kind: CheckKind::Axial,
                    ratio: 0.5,
                    detail: "F=335".into(),
                }],
            });
        }
        for (analysis, design, keeps_analysis) in
            [(14, 5, false), (ANALYSIS_CONTRACT_VERSION, 5, true)]
        {
            let (mut loaded, path) = restore(
                &app,
                Some(analysis),
                Some(design),
                &format!("sm520-{t}-{analysis}-{design}"),
            );
            assert_eq!(
                loaded
                    .core
                    .scoped
                    .results
                    .as_ref()
                    .unwrap()
                    .has_analysis_results(),
                keeps_analysis
            );
            assert!(!loaded
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .has_design_results());
            loaded.select_displayed_result(StaticKey::Case(StaticCaseKey::User(LoadCaseId(0))));
            loaded.save_project_to(path.clone());
            let mut reopened = App::default();
            reopened.open_project_from(path.clone());
            assert!(!reopened
                .core
                .scoped
                .results
                .as_ref()
                .is_some_and(|r| r.has_design_results()));
            assert!(!crate::summary::build_report_csv(&reopened).contains("旧SM520成功335"));
            if keeps_analysis {
                reopened.run_design_check();
            } else {
                reopened.run_linear_static(LoadCaseId(0));
            }
            let bundle = reopened.core.scoped.results.as_ref().unwrap();
            assert_eq!(
                bundle.analysis_contract_version,
                Some(ANALYSIS_CONTRACT_VERSION)
            );
            assert_eq!(
                bundle.design_contract_version,
                Some(DESIGN_CONTRACT_VERSION)
            );
            assert!(bundle.has_design_results());
            if t > 100.0 {
                assert!(bundle.member_checks.iter().flat_map(|m| &m.positions).all(|p| matches!(&p.outcome, CheckOutcome::Skipped { reason } if reason.contains("適用範囲"))));
            }
            std::fs::remove_file(path).unwrap();
        }
    }
}
