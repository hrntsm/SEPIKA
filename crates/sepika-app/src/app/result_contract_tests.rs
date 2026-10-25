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

#[test]
fn 旧解析契約39の成果は読込時に失効し再計算で現行版になる() {
    let app = calculated();
    let (mut loaded, path) = restore(&app, Some(39), Some(25), "772-analysis39");
    let stale = loaded.core.scoped.results.as_ref().unwrap();
    assert!(!stale.has_analysis_results());
    assert!(!stale.has_design_results());
    assert!(loaded.core.scoped.staleness.results_stale);
    assert!(loaded.core.scoped.last_static.is_none());
    loaded.run_linear_static(LoadCaseId(0));
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
    let fresh = loaded.core.scoped.results.as_ref().unwrap();
    assert!(fresh.has_analysis_results());
    assert_eq!(
        fresh.analysis_contract_version,
        Some(ANALYSIS_CONTRACT_VERSION)
    );
    assert_eq!(fresh.design_contract_version, Some(DESIGN_CONTRACT_VERSION));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn panel_761_旧解析契約40の成果は読込時に失効し再計算で現行版になる() {
    let app = panel_757_calculated();
    let (mut loaded, path) = restore(
        &app,
        Some(40),
        Some(DESIGN_CONTRACT_VERSION),
        "761-analysis40",
    );
    let stale = loaded.core.scoped.results.as_ref().unwrap();
    assert!(!stale.has_analysis_results());
    assert!(!stale.has_design_results());
    assert!(loaded.core.scoped.staleness.results_stale);
    assert!(loaded.core.scoped.last_static.is_none());
    loaded.run_linear_static(LoadCaseId(0));
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
    let fresh = loaded.core.scoped.results.as_ref().unwrap();
    assert!(fresh.has_analysis_results());
    assert_eq!(
        fresh.analysis_contract_version,
        Some(ANALYSIS_CONTRACT_VERSION)
    );
    assert_eq!(fresh.design_contract_version, Some(DESIGN_CONTRACT_VERSION));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn fiber_783_旧解析契約41の成果は読込時に失効し再計算で現行版になる() {
    let app = panel_757_calculated();
    let (mut loaded, path) = restore(
        &app,
        Some(41),
        Some(DESIGN_CONTRACT_VERSION),
        "783-analysis41",
    );
    let stale = loaded.core.scoped.results.as_ref().unwrap();
    assert!(!stale.has_analysis_results());
    assert!(!stale.has_design_results());
    assert!(loaded.core.scoped.staleness.results_stale);
    assert!(loaded.core.scoped.last_static.is_none());
    loaded.run_linear_static(LoadCaseId(0));
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
    let fresh = loaded.core.scoped.results.as_ref().unwrap();
    assert!(fresh.has_analysis_results());
    assert_eq!(
        fresh.analysis_contract_version,
        Some(ANALYSIS_CONTRACT_VERSION)
    );
    assert_eq!(fresh.design_contract_version, Some(DESIGN_CONTRACT_VERSION));
    std::fs::remove_file(path).unwrap();
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
        ("old-local-frame-reference", Some(37), "旧契約"),
        ("old-face-direction-reference", Some(38), "旧契約"),
        ("old-secondary-materialization", Some(44), "旧契約"),
        ("old-pushover-support-resistance", Some(48), "旧契約"),
        ("old-fiber-end-release-equilibrium", Some(49), "旧契約"),
        ("old-checkpoint-and-fixed-time-history", Some(50), "旧契約"),
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
    for version in [5, 6, 7, 20, 22, 24, 26, 28, 37, 38, 49, 50] {
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
        ("design-old-steel-f-selection", Some(22)),
        ("design-old-local-frame", Some(23)),
        ("design-old-face-direction", Some(24)),
        ("design-old-secondary-materialization", Some(27)),
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

pub(super) fn direct_secondary_beam_app() -> App {
    let mut model = super::tests::make_floor_secondary_beam_test_model();
    let material_id = sepika_core::ids::MaterialId(model.materials.len() as u32);
    let mut concrete = crate::sample::portal_frame().materials[0].clone();
    concrete.id = material_id;
    concrete.category = sepika_core::model::MaterialCategory::Concrete;
    concrete.fc = Some(24.0);
    concrete.density = 2.4e-9;
    model.materials.push(concrete);
    let section_id = SectionId(model.sections.len() as u32);
    let mut slab_section = sepika_core::section_shape::SectionShape::RcSlab { thickness: 100.0 }
        .to_section(section_id, "床検定用スラブ".into());
    slab_section.material = Some(material_id);
    model.sections.push(slab_section);
    for slab in &mut model.slabs {
        slab.plate.section = Some(section_id);
    }
    let mut app = App::default();
    app.load_model(model);
    app
}

#[test]
fn ovika_old_direct_secondary_beam_design_requires_explicit_recheck() {
    let mut app = direct_secondary_beam_app();
    for region in &mut app.core.model.floor_regions {
        for beam in &mut region.secondary_beams {
            beam.vertical_eccentricity = Some(-500.);
        }
    }
    for beam in &mut app.core.model.unassigned_beams {
        beam.vertical_eccentricity = Some(-500.);
    }
    assert!(
        sepika_load::self_weight::all_beam_upstand_segments(&app.core.model)
            .unwrap()
            .iter()
            .any(|segment| matches!(
                segment.beam,
                sepika_core::model::SupportMemberId::Secondary(_)
            ) && segment.design_weight_n > 0.)
    );
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let original = app.current_design_results().expect("直接小梁検定");
    assert_eq!(
        (original.beam_checks.len(), original.slab_checks.len()),
        (1, 2)
    );
    assert_eq!(original.analysis_contract_version, None);
    assert_eq!(original.design_source, None);
    assert_eq!(
        original.design_contract_version,
        Some(DESIGN_CONTRACT_VERSION)
    );
    let original_model = rmp_serde::to_vec_named(&app.core.model).unwrap();

    for version in [23, 24, 29] {
        let (mut loaded, path) = restore(
            &app,
            None,
            Some(version),
            &format!("old-direct-secondary-design{version}"),
        );
        assert_eq!(
            rmp_serde::to_vec_named(&loaded.core.model).unwrap(),
            original_model
        );
        let old = loaded.core.scoped.results.as_ref().unwrap();
        assert_eq!(old.analysis_contract_version, None);
        assert_eq!(old.design_source, None);
        assert!(old.beam_checks.is_empty() && old.slab_checks.is_empty());
        assert!(loaded.current_design_results().is_none());
        assert!(loaded.require_design_input().is_err());
        assert!(loaded.core.scoped.staleness.design_stale);
        assert!(loaded
            .core
            .scoped
            .result_contract_notice
            .as_ref()
            .is_some_and(|notice| notice.contains("旧契約")));

        loaded.run_design_check();
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "{:?}",
            loaded.core.scoped.last_error
        );
        let current = loaded.current_design_results().expect("明示再検定");
        assert_eq!(
            (current.beam_checks.len(), current.slab_checks.len()),
            (1, 2)
        );
        assert_eq!(current.analysis_contract_version, None);
        assert_eq!(current.design_source, None);
        assert_eq!(
            current.design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn direct_secondary_beam_design_preserves_demand_for_large_finite_reference() {
    let mut app = direct_secondary_beam_app();
    let primary = &app.core.model.elements[0];
    let start = app.core.model.nodes[primary.nodes[0].index()].coord;
    let end = app.core.model.nodes[primary.nodes[1].index()].coord;
    assert_eq!(start, [0.0, 0.0, 0.0]);
    assert_eq!(end, [4000.0, 0.0, 0.0]);
    app.core.model.elements[0].local_axis.ref_vector = [0.0, 1.0, 0.0];
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let unit = app.current_design_results().expect("単位Y参照の直接検定");
    assert_eq!((unit.beam_checks.len(), unit.slab_checks.len()), (1, 2));
    let unit_beam = unit.beam_checks[0].2.clone();
    assert!(!unit_beam.unchecked);
    assert!(unit_beam.m_max > 0.0 && unit_beam.q_max > 0.0);

    app.core.model.elements[0].local_axis.ref_vector = [0.0, 1.0e308, 0.0];
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let large = app.current_design_results().expect("極大Y参照の直接検定");
    assert_eq!((large.beam_checks.len(), large.slab_checks.len()), (1, 2));
    assert_eq!(large.beam_checks[0].2, unit_beam);

    app.core.model.elements[0].local_axis.ref_vector = [f64::NAN, 1.0, 0.0];
    app.run_design_check();
    let error = app
        .core
        .scoped
        .last_error
        .as_deref()
        .expect("必要支持材の非有限参照");
    assert!(
        error.contains("二次部材 0")
            && error.contains("支持材 Primary(ElemId(0))")
            && error.contains("非有限"),
        "{error}"
    );
    assert!(app.current_design_results().is_none());
    let failed = app.core.scoped.results.as_ref().unwrap();
    assert!(failed.beam_checks.is_empty() && failed.slab_checks.is_empty());

    app.core.model.elements[0].local_axis.ref_vector = [0.0, 1.0, 0.0];
    app.core.scoped.last_error = None;
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let recovered = app
        .current_design_results()
        .expect("参照方向復旧後の直接検定");
    assert_eq!(
        (recovered.beam_checks.len(), recovered.slab_checks.len()),
        (1, 2)
    );
    assert_eq!(recovered.beam_checks[0].2, unit_beam);
}

#[test]
fn direct_secondary_beam_design_ignores_unused_primary_local_reference() {
    let mut app = direct_secondary_beam_app();
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let expected = app.current_design_results().expect("直接小梁検定");
    assert_eq!(
        (expected.beam_checks.len(), expected.slab_checks.len()),
        (1, 2)
    );
    let expected_beam = expected.beam_checks[0].2.clone();

    let mut a = app.core.model.nodes[0].clone();
    a.id = NodeId(6);
    a.coord = [10000.0, 0.0, 0.0];
    let mut b = app.core.model.nodes[1].clone();
    b.id = NodeId(7);
    b.coord = [14000.0, 0.0, 0.0];
    app.core.model.nodes.extend([a, b]);
    let mut unused = app.core.model.elements[0].clone();
    unused.id = ElemId(4);
    unused.nodes = [NodeId(6), NodeId(7)].into_iter().collect();
    unused.local_axis.ref_vector = [f64::NAN, 0.0, 0.0];
    app.core.model.elements.push(unused);
    app.core
        .model
        .validate_reference_ids()
        .expect("独立一次梁の参照整合");

    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let current = app
        .current_design_results()
        .expect("独立梁追加後の直接小梁検定");
    assert_eq!(
        (current.beam_checks.len(), current.slab_checks.len()),
        (1, 2)
    );
    assert_eq!(current.beam_checks[0].2, expected_beam);
}

#[test]
fn direct_secondary_beam_design_rejects_used_finite_unresolvable_local_frame() {
    let mut app = direct_secondary_beam_app();
    let rotation = std::f64::consts::FRAC_1_SQRT_2;
    for node in &mut app.core.model.nodes {
        let [x, y, z] = node.coord;
        node.coord = [(x - y) * rotation, (x + y) * rotation, z];
    }
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let baseline = app.current_design_results().expect("回転後の直接小梁検定");
    assert_eq!(
        (baseline.beam_checks.len(), baseline.slab_checks.len()),
        (1, 2)
    );

    let original = app.core.model.elements[0].local_axis.ref_vector;
    let reference: [f64; 3] = [1e100, 1e100 + 2e84, 0.0];
    assert!(reference.iter().all(|value| value.is_finite()));
    let support = &app.core.model.elements[0];
    let i = app.core.model.nodes[support.nodes[0].index()].coord;
    let j = app.core.model.nodes[support.nodes[1].index()].coord;
    assert!(sepika_core::geom::vec3::local_frame_axes(i, j, reference).is_err());
    app.core.model.elements[0].local_axis.ref_vector = reference;
    app.run_design_check();
    let error = app
        .core
        .scoped
        .last_error
        .as_deref()
        .expect("必要支持材の基底不能");
    assert!(
        error.contains("二次部材 0")
            && error.contains("支持材 Primary(ElemId(0))")
            && error.contains("単位直交基底"),
        "{error}"
    );
    assert!(app.current_design_results().is_none());
    let failed = app.core.scoped.results.as_ref().unwrap();
    assert!(failed.beam_checks.is_empty() && failed.slab_checks.is_empty());

    app.core.model.elements[0].local_axis.ref_vector = original;
    app.core.scoped.last_error = None;
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let recovered = app.current_design_results().expect("支持材参照復旧");
    assert_eq!(
        (recovered.beam_checks.len(), recovered.slab_checks.len()),
        (1, 2)
    );
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
fn member_load_gate_expires_analysis30_and_keeps_current_results() {
    let app = calculated();
    for (version, keep) in [(30, false), (ANALYSIS_CONTRACT_VERSION, true)] {
        let (loaded, path) = restore(
            &app,
            Some(version),
            Some(DESIGN_CONTRACT_VERSION),
            &format!("member-load713-contract-{version}"),
        );
        let results = loaded.core.scoped.results.as_ref().unwrap();
        assert_eq!(results.has_analysis_results(), keep);
        assert_eq!(results.has_design_results(), keep);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn member_history_gate_expires_analysis32_and_keeps_current_results() {
    let app = calculated();
    for (version, keep) in [(32, false), (ANALYSIS_CONTRACT_VERSION, true)] {
        let (loaded, path) = restore(
            &app,
            Some(version),
            Some(DESIGN_CONTRACT_VERSION),
            &format!("member-history732-contract-{version}"),
        );
        let results = loaded.core.scoped.results.as_ref().unwrap();
        assert_eq!(results.has_analysis_results(), keep);
        assert_eq!(results.has_design_results(), keep);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn beam_stiffness_gate_expires_analysis33_and_keeps_current_results() {
    let app = calculated();
    for (version, keep) in [(33, false), (ANALYSIS_CONTRACT_VERSION, true)] {
        let (loaded, path) = restore(
            &app,
            Some(version),
            Some(DESIGN_CONTRACT_VERSION),
            &format!("beam736-contract-{version}"),
        );
        let results = loaded.core.scoped.results.as_ref().unwrap();
        assert_eq!(results.has_analysis_results(), keep);
        assert_eq!(results.has_design_results(), keep);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn panel_g_gate_expires_analysis31_and_keeps_current_results() {
    let app = calculated();
    for (version, keep) in [(31, false), (ANALYSIS_CONTRACT_VERSION, true)] {
        let (loaded, path) = restore(
            &app,
            Some(version),
            Some(DESIGN_CONTRACT_VERSION),
            &format!("panel728-contract-{version}"),
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
    for fy in [None, Some(177.0), Some(235.0), Some(335.0), Some(999.0)] {
        let mut app = sm520_app(80.0, fy);
        for section in &mut app.core.model.sections {
            if let Some(sepika_core::section_shape::SectionShape::SteelH { flange_thick, .. }) =
                section.shape.as_mut()
            {
                *flange_thick = 100.001;
            }
        }
        for _ in 0..2 {
            app.run_design_check();
            let reason = app.core.scoped.last_error.as_deref().unwrap();
            assert!(
                reason.contains("部材 0") && reason.contains("適用範囲"),
                "{reason}"
            );
            let result = app.core.scoped.results.as_ref().unwrap();
            assert!(!result.has_design_results());
            assert!(result.design_contract_version.is_none());
            assert!(app.current_design_results().is_none());
            assert!(!crate::summary::build_report_csv(&app).contains("全適合"));
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
        assert!(loaded.current_design_results().is_none());
        assert!(!crate::summary::build_report_csv(&loaded).contains("全適合"));
        loaded.select_displayed_result(StaticKey::Case(StaticCaseKey::User(LoadCaseId(0))));
        assert!(!crate::summary::build_report_csv(&loaded).contains("全適合"));
        loaded.run_linear_static(LoadCaseId(0));
        let reason = loaded.core.scoped.last_error.as_deref().unwrap();
        assert!(
            reason.contains("部材 0") && reason.contains("適用範囲"),
            "{reason}"
        );
        assert!(loaded.current_design_results().is_none());
        std::fs::remove_file(path).unwrap();
    }
}
#[test]
fn sm520_old_335_success_expires_without_save_or_display_reauthentication() {
    use sepika_design_jp::{CheckComponent, CheckKind, CheckOutcome, CheckResult};
    for t in [80.0] {
        let mut app = sm520_app(t, Some(335.0));
        let old_success = CheckOutcome::Checked(CheckResult {
            basis: "旧SM520成功335".into(),
            detail: String::new(),
            components: vec![CheckComponent {
                kind: CheckKind::Axial,
                ratio: 0.5,
                detail: "F=335".into(),
            }],
        });
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
            position.outcome = old_success.clone();
        }
        for joint in &mut app.core.scoped.results.as_mut().unwrap().joint_checks {
            joint.outcome = old_success.clone();
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
            std::fs::remove_file(path).unwrap();
        }
    }
}

#[test]
fn panel_753_yield_gate_expires_analysis35_and_keeps_current_results() {
    let app = calculated();
    for (version, keep) in [(35, false), (ANALYSIS_CONTRACT_VERSION, true)] {
        let (loaded, path) = restore(
            &app,
            Some(version),
            Some(DESIGN_CONTRACT_VERSION),
            &format!("panel753-contract-{version}"),
        );
        let results = loaded.core.scoped.results.as_ref().unwrap();
        assert_eq!(results.has_analysis_results(), keep);
        assert_eq!(results.has_design_results(), keep);
        std::fs::remove_file(path).unwrap();
    }
}

fn panel_757_calculated() -> App {
    let mut model = crate::sample::portal_frame();
    let (sin, cos) = 0.37_f64.sin_cos();
    for element in &mut model.elements {
        if element.section == Some(sepika_core::ids::SectionId(0)) {
            element.local_axis.ref_vector = [cos, sin, 0.0];
        }
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
    let bundle = app.core.scoped.results.as_ref().unwrap();
    assert!(!bundle.panel_moments.is_empty());
    assert!(bundle
        .panel_moments
        .iter()
        .any(|(_, m)| m.iter().any(|x| x.abs() > 1.0)));
    assert!(bundle.has_design_results());
    app
}

fn assert_panel_758_both_axes(app: &App) {
    let checks = &app.current_design_results().unwrap().joint_checks;
    for node in [NodeId(2), NodeId(3)] {
        for axis in ["強軸", "弱軸"] {
            let label = format!("パネルゾーン(S) {axis}");
            let matching: Vec<_> = checks
                .iter()
                .filter(|check| check.node == node && check.label == label)
                .collect();
            assert_eq!(matching.len(), 1, "節点 {} {axis}", node.0);
            let sepika_design_jp::CheckOutcome::Checked(result) = &matching[0].outcome else {
                panic!("節点 {} {axis}: {:?}", node.0, matching[0].outcome);
            };
            assert!(result.ratio().is_finite() && result.ratio() > 0.0);
            assert_eq!(result.components.len(), 1);
            assert!(matches!(
                result.components[0].kind,
                sepika_design_jp::CheckKind::Shear
            ));
        }
    }
    assert_eq!(checks.len(), 4);
    let csv = crate::summary::build_report_csv(app);
    assert!(csv.contains("パネルゾーン(S) 強軸"));
    assert!(csv.contains("パネルゾーン(S) 弱軸"));
}

#[test]
fn panel_758_design21_expires_checks_and_display_but_keeps_current_analysis_until_recheck() {
    let app = panel_757_calculated();
    assert_panel_758_both_axes(&app);
    let original = app.core.scoped.results.as_ref().unwrap();
    let expected_checks = rmp_serde::to_vec_named(&original.joint_checks).unwrap();
    let (mut loaded, path) = restore(
        &app,
        Some(ANALYSIS_CONTRACT_VERSION),
        Some(21),
        "panel758-design21",
    );
    for pass in 0..2 {
        let results = loaded.core.scoped.results.as_ref().unwrap();
        assert_eq!(
            results.analysis_contract_version,
            Some(ANALYSIS_CONTRACT_VERSION)
        );
        assert_eq!(results.panel_moments, original.panel_moments);
        assert_eq!(results.member_forces, original.member_forces);
        assert_eq!(results.statics[0].1.disp, original.statics[0].1.disp);
        assert!(!results.has_design_results());
        assert!(results.design_source.is_none());
        assert!(!results
            .input_records
            .iter()
            .any(|(key, _)| *key == ResultInputKey::Design));
        assert!(loaded.require_selected_static_input().is_ok());
        assert!(loaded.current_design_results().is_none());
        assert!(!loaded.core.scoped.staleness.results_stale);
        let csv = crate::summary::build_report_csv(&loaded);
        if pass == 0 {
            assert!(loaded.core.scoped.staleness.design_stale);
            assert!(
                csv.contains("旧契約")
                    && csv.contains("保存版=21")
                    && csv.contains(&format!("現行版={DESIGN_CONTRACT_VERSION}"))
            );
        }
        assert!(!csv.contains("パネルゾーン(S) 強軸"));
        assert!(!csv.contains("パネルゾーン(S) 弱軸"));
        loaded.select_displayed_result(app.core.scoped.last_static.unwrap());
        loaded.core.scoped.staleness.mark_fresh();
        assert!(loaded.current_design_results().is_none());
        loaded.save_project_to(path.clone());
        assert!(loaded.core.scoped.last_error.is_none());
        let mut reopened = App::default();
        reopened.open_project_from(path.clone());
        assert!(reopened.core.scoped.last_error.is_none());
        assert_eq!(
            reopened
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .design_contract_version,
            None
        );
        loaded = reopened;
    }
    loaded.run_design_check();
    assert!(loaded.core.scoped.last_error.is_none());
    assert_panel_758_both_axes(&loaded);
    let results = loaded.core.scoped.results.as_ref().unwrap();
    assert_eq!(
        results.design_contract_version,
        Some(DESIGN_CONTRACT_VERSION)
    );
    assert_eq!(results.panel_moments, original.panel_moments);
    assert_eq!(
        rmp_serde::to_vec_named(&results.joint_checks).unwrap(),
        expected_checks
    );
    assert!(loaded.core.scoped.result_contract_notice.is_none());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn panel_758_current_design_save_open_display_and_recheck_preserve_both_principal_axes() {
    let mut app = panel_757_calculated();
    assert_panel_758_both_axes(&app);
    let original = app.core.scoped.results.as_ref().unwrap().clone();
    let expected_checks = rmp_serde::to_vec_named(&original.joint_checks).unwrap();
    let expected_csv = crate::summary::build_report_csv(&app);
    let path =
        std::env::temp_dir().join(format!("sepika-758-current-{}.ovika", std::process::id()));
    for _ in 0..2 {
        app.save_project_to(path.clone());
        assert!(app.core.scoped.last_error.is_none());
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert!(loaded.core.scoped.last_error.is_none());
        loaded.select_displayed_result(app.core.scoped.last_static.unwrap());
        assert_panel_758_both_axes(&loaded);
        assert!(!loaded.core.scoped.staleness.results_stale);
        assert!(!loaded.core.scoped.staleness.design_stale);
        let results = loaded.current_design_results().unwrap();
        assert_eq!(
            results.analysis_contract_version,
            Some(ANALYSIS_CONTRACT_VERSION)
        );
        assert_eq!(
            results.design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
        assert_eq!(results.panel_moments, original.panel_moments);
        assert_eq!(results.member_forces, original.member_forces);
        assert_eq!(
            rmp_serde::to_vec_named(&results.joint_checks).unwrap(),
            expected_checks
        );
        assert_eq!(crate::summary::build_report_csv(&loaded), expected_csv);
        loaded.run_design_check();
        assert!(loaded.core.scoped.last_error.is_none());
        assert_panel_758_both_axes(&loaded);
        assert_eq!(
            rmp_serde::to_vec_named(&loaded.current_design_results().unwrap().joint_checks)
                .unwrap(),
            expected_checks
        );
        app = loaded;
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn panel_758_rotated_column_retains_distinct_invalid_plane_reasons_across_current_save_open() {
    use sepika_core::{dof::Dof6Mask, ids::SectionId, section_shape::SectionShape};
    use sepika_design_jp::CheckOutcome;
    let mut model = crate::sample::portal_frame();
    model.panel_zone = sepika_core::model::PanelZoneMode::None;
    let (sin, cos) = 0.37_f64.sin_cos();
    for column in &mut model.elements[..2] {
        column.local_axis.ref_vector = [cos, sin, 0.0];
    }
    for (id, height) in [(2, 500.0), (3, 600.0)] {
        let mut section = SectionShape::SteelH {
            height,
            width: 200.0,
            web_thick: 8.0,
            flange_thick: 13.0,
            root_r: Some(0.0),
        }
        .to_section(SectionId(id), format!("H{height}"));
        section.material = model.sections[1].material;
        section.frame_use = model.sections[1].frame_use;
        model.sections.push(section);
    }
    for (coord, section) in [
        ([-6000.0, 0.0, 3500.0], 2),
        ([0.0, 6000.0, 3500.0], 1),
        ([0.0, -6000.0, 3500.0], 3),
        ([6000.0, 6000.0, 3500.0], 1),
    ] {
        let mut node = model.nodes[2].clone();
        node.id = NodeId(model.nodes.len() as u32);
        node.coord = coord;
        node.restraint = Dof6Mask::FIXED;
        let mut beam = model.elements[2].clone();
        beam.id = ElemId(model.elements.len() as u32);
        beam.nodes = smallvec::smallvec![NodeId(2), node.id];
        beam.section = Some(SectionId(section));
        model.nodes.push(node);
        model.elements.push(beam);
    }
    let mut app = App::default();
    app.load_model(model);
    app.core.analysis_cfg.threads = 1;
    app.run_linear_static(LoadCaseId(0));
    assert!(app.core.scoped.last_error.is_none());
    let original = app.current_design_results().unwrap().clone();
    let rejected: Vec<_> = original
        .joint_checks
        .iter()
        .filter(|c| c.node == NodeId(2))
        .collect();
    for axis in ["強軸", "弱軸"] {
        assert!(rejected.iter().any(|c| c.label == format!("パネルゾーン(S) {axis}")
            && matches!(&c.outcome, CheckOutcome::Skipped { reason } if reason.contains("段違い"))));
    }
    for other_depth in ["db=487 mm", "db=587 mm"] {
        assert!(
            rejected
                .iter()
                .any(|c| matches!(&c.outcome, CheckOutcome::Skipped { reason }
            if reason.contains("db=387 mm") && reason.contains(other_depth))),
            "{other_depth}"
        );
    }
    let path = std::env::temp_dir().join(format!(
        "sepika-758-multiple-gates-{}.ovika",
        std::process::id()
    ));
    app.save_project_to(path.clone());
    assert!(app.core.scoped.last_error.is_none());
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    assert!(loaded.core.scoped.last_error.is_none());
    assert!(!loaded.core.scoped.staleness.results_stale);
    assert!(!loaded.core.scoped.staleness.design_stale);
    let restored = loaded.current_design_results().unwrap();
    assert_eq!(restored.member_forces, original.member_forces);
    assert_eq!(restored.statics[0].1.disp, original.statics[0].1.disp);
    assert_eq!(
        rmp_serde::to_vec_named(&restored.joint_checks).unwrap(),
        rmp_serde::to_vec_named(&original.joint_checks).unwrap()
    );
    assert_eq!(
        crate::summary::build_report_csv(&loaded),
        crate::summary::build_report_csv(&app)
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn panel_757_rotated_panel_results_expire_analysis36_and_survive_current_roundtrip() {
    let app = panel_757_calculated();
    let original = app.core.scoped.results.as_ref().unwrap();
    for (version, keep) in [(36, false), (ANALYSIS_CONTRACT_VERSION, true)] {
        let (mut loaded, path) = restore(
            &app,
            Some(version),
            Some(DESIGN_CONTRACT_VERSION),
            &format!("panel757-contract-{version}"),
        );
        let results = loaded.core.scoped.results.as_ref().unwrap();
        assert_eq!(results.has_analysis_results(), keep);
        assert_eq!(results.has_design_results(), keep);
        assert_eq!(loaded.core.scoped.staleness.results_stale, !keep);
        assert_eq!(loaded.core.scoped.staleness.design_stale, !keep);
        if keep {
            assert_eq!(results.panel_moments, original.panel_moments);
            assert_eq!(results.statics[0].1.disp, original.statics[0].1.disp);
        } else {
            assert!(results.panel_moments.is_empty());
            assert!(loaded.core.scoped.last_static.is_none());
            assert!(loaded.core.scoped.staleness.last_run.is_none());
        }
        loaded.select_displayed_result(app.core.scoped.last_static.unwrap());
        loaded.save_project_to(path.clone());
        let mut reopened = App::default();
        reopened.open_project_from(path.clone());
        assert!(reopened.core.scoped.last_error.is_none());
        assert_eq!(
            reopened
                .core
                .scoped
                .results
                .as_ref()
                .is_some_and(|r| r.has_analysis_results()),
            keep
        );
        if keep {
            assert_eq!(
                reopened.core.scoped.results.as_ref().unwrap().panel_moments,
                original.panel_moments
            );
        }
        reopened.run_linear_static(LoadCaseId(0));
        assert!(
            reopened.core.scoped.last_error.is_none(),
            "{:?}",
            reopened.core.scoped.last_error
        );
        let recalculated = reopened.core.scoped.results.as_ref().unwrap();
        assert_eq!(
            recalculated.analysis_contract_version,
            Some(ANALYSIS_CONTRACT_VERSION)
        );
        assert_eq!(
            recalculated.design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
        assert_eq!(recalculated.panel_moments, original.panel_moments);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn panel_759_saved_analysis46_requires_reanalysis_and_current_contract_roundtrips() {
    let app = panel_757_calculated();
    for (version, keep) in [(46, false), (ANALYSIS_CONTRACT_VERSION, true)] {
        let (mut loaded, path) = restore(
            &app,
            Some(version),
            Some(DESIGN_CONTRACT_VERSION),
            &format!("panel759-contract-{version}"),
        );
        let results = loaded.core.scoped.results.as_ref().unwrap();
        assert_eq!(results.has_analysis_results(), keep);
        assert_eq!(results.has_design_results(), keep);
        assert_eq!(loaded.core.scoped.staleness.results_stale, !keep);
        assert_eq!(loaded.core.scoped.staleness.design_stale, !keep);
        loaded.run_preparation();
        loaded.run_linear_static(LoadCaseId(0));
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
                .analysis_contract_version,
            Some(ANALYSIS_CONTRACT_VERSION)
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn panel_757_preparation_roundtrip_preserves_both_axes_and_old_scalar_requires_repreparation() {
    let app = panel_757_calculated();
    let prep = app.core.scoped.preparation.as_ref().unwrap();
    assert!(!prep.panels.is_empty());
    for legacy in [false, true] {
        let mut value = serde_json::to_value(prep).unwrap();
        if legacy {
            for panel in value["panels"].as_array_mut().unwrap() {
                for field in ["dc", "db", "tp", "ve", "k_panel"] {
                    panel[field] = panel[field][0].clone();
                }
            }
        }
        let bytes = rmp_serde::to_vec_named(&value).unwrap();
        let path = std::env::temp_dir().join(format!(
            "sepika-757-prep-{}-{legacy}.ovika",
            std::process::id()
        ));
        sepika_io::ovika::save_ovika(
            &path,
            &app.core.model,
            sepika_io::ovika::OvikaExtras {
                preparation: Some(&bytes),
                ..Default::default()
            },
        )
        .unwrap();
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert!(loaded.core.scoped.last_error.is_none());
        assert_eq!(loaded.core.scoped.staleness.preparation_stale, legacy);
        if legacy {
            assert!(loaded.core.scoped.preparation.is_none());
            assert!(loaded
                .core
                .scoped
                .last_notice
                .as_ref()
                .is_some_and(|n| n.contains("準備計算の結果") && n.contains("再実行")));
            loaded.run_preparation();
            assert!(loaded.core.scoped.last_error.is_none());
        }
        let restored = loaded.core.scoped.preparation.as_ref().unwrap();
        assert_eq!(restored.panels.len(), prep.panels.len());
        for (actual, expected) in restored.panels.iter().zip(&prep.panels) {
            assert_eq!(actual.node, expected.node);
            assert_eq!(actual.dc, expected.dc);
            assert_eq!(actual.db, expected.db);
            assert_eq!(actual.tp, expected.tp);
            assert_eq!(actual.ve, expected.ve);
            assert_eq!(actual.k_panel, expected.k_panel);
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn steel601_old_design22_preserves_current_analysis_and_requires_explicit_recheck() {
    let mut model = crate::sample::portal_frame();
    model.panel_zone = sepika_core::model::PanelZoneMode::None;
    model.materials[0].name = "SS400".into();
    model.materials[0].fy = Some(235.0);
    for section in &mut model.sections {
        let mut actual = sepika_core::section_shape::SectionShape::SteelH {
            height: section.depth,
            width: section.width,
            web_thick: 10.0,
            flange_thick: 50.0,
            root_r: Some(0.0),
        }
        .to_section(section.id, section.name.clone());
        actual.material = section.material;
        actual.frame_use = section.frame_use;
        *section = actual;
    }
    let mut app = App::default();
    app.load_model(crate::app::tests::with_explicit_pushover_gravity(model));
    app.core.analysis_cfg.threads = 1;
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let original = app
        .current_design_results()
        .expect("表Fで算定した検定")
        .clone();
    let checks = rmp_serde::to_vec_named(&original.member_checks).unwrap();
    assert!(!original.member_checks.is_empty());
    assert_eq!(
        original.analysis_contract_version,
        Some(ANALYSIS_CONTRACT_VERSION)
    );
    assert_eq!(
        original.design_contract_version,
        Some(DESIGN_CONTRACT_VERSION)
    );
    let (mut loaded, path) = restore(
        &app,
        Some(ANALYSIS_CONTRACT_VERSION),
        Some(22),
        "steel601-old-design22",
    );
    let result = loaded.core.scoped.results.as_ref().unwrap();
    assert_eq!(result.member_forces, original.member_forces);
    assert_eq!(result.statics[0].1.disp, original.statics[0].1.disp);
    assert!(!result.has_design_results());
    assert!(loaded.current_design_results().is_none());
    loaded.select_displayed_result(app.core.scoped.last_static.unwrap());
    loaded.core.scoped.staleness.mark_fresh();
    assert!(loaded.current_design_results().is_none());
    assert!(!crate::summary::build_report_csv(&loaded).contains("全適合"));
    loaded.save_project_to(path.clone());
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert!(reopened.core.scoped.last_error.is_none());
    assert!(reopened.current_design_results().is_none());
    assert_eq!(
        reopened.core.scoped.results.as_ref().unwrap().member_forces,
        original.member_forces
    );
    reopened.run_design_check();
    assert!(
        reopened.core.scoped.last_error.is_none(),
        "{:?}",
        reopened.core.scoped.last_error
    );
    let current = reopened.current_design_results().unwrap();
    assert_eq!(
        current.design_contract_version,
        Some(DESIGN_CONTRACT_VERSION)
    );
    assert_eq!(
        rmp_serde::to_vec_named(&current.member_checks).unwrap(),
        checks
    );
    reopened.save_project_to(path.clone());
    let mut current_loaded = App::default();
    current_loaded.open_project_from(path.clone());
    let current = current_loaded.current_design_results().unwrap();
    assert_eq!(
        current.analysis_contract_version,
        Some(ANALYSIS_CONTRACT_VERSION)
    );
    assert_eq!(
        current.design_contract_version,
        Some(DESIGN_CONTRACT_VERSION)
    );
    assert_eq!(current.member_forces, original.member_forces);
    assert_eq!(
        rmp_serde::to_vec_named(&current.member_checks).unwrap(),
        checks
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn steel601_failed_recheck_clears_old_design_and_keeps_valid_analysis() {
    let app = calculated();
    for (name, thickness_mm) in [
        ("missing-thickness", None),
        ("invalid-thickness", Some(-1.0)),
        ("out-of-range", Some(101.0)),
    ] {
        let (mut loaded, path) = restore(
            &app,
            Some(ANALYSIS_CONTRACT_VERSION),
            Some(DESIGN_CONTRACT_VERSION),
            &format!("steel601-{name}"),
        );
        let original = loaded.current_design_results().unwrap().clone();
        loaded.core.model.materials[0].name = "SS400".into();
        loaded.core.model.sections[0].shape = None;
        loaded.core.model.sections[0].thickness = thickness_mm;
        loaded.run_design_check();
        let reason = loaded.core.scoped.last_error.as_deref().unwrap();
        assert!(reason.contains("部材 0"), "{reason}");
        assert!(
            reason.contains(if thickness_mm == Some(101.0) {
                "適用範囲"
            } else {
                "板厚"
            }),
            "{reason}"
        );
        let result = loaded.core.scoped.results.as_ref().unwrap();
        assert_eq!(result.member_forces, original.member_forces);
        assert!(!result.has_design_results());
        assert_eq!(result.design_contract_version, None);
        assert!(result.design_source.is_none());
        loaded.core.scoped.staleness.mark_fresh();
        assert!(loaded.current_design_results().is_none());
        assert!(!crate::summary::build_report_csv(&loaded).contains("全適合"));
        loaded.save_project_to(path.clone());
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "{name}: {:?}",
            loaded.core.scoped.last_error
        );
        let mut reopened = App::default();
        reopened.open_project_from(path.clone());
        assert!(reopened.core.scoped.last_error.is_none());
        if let Some(result) = reopened.core.scoped.results.as_ref() {
            assert!(result.member_checks.is_empty());
            assert!(result.wall_checks.is_empty());
            assert!(result.beam_checks.is_empty());
            assert!(result.slab_checks.is_empty());
            assert!(result.joint_checks.iter().all(|check| {
                check.label.starts_with("パネルゾーン(S)")
                    && matches!(
                        check.outcome,
                        sepika_design_jp::CheckOutcome::Skipped { .. }
                    )
            }));
            assert!(result.design_contract_version.is_none());
        }
        assert!(reopened.core.scoped.staleness.design_stale);
        assert!(reopened.current_design_results().is_none());
        assert!(!crate::summary::build_report_csv(&reopened).contains("全適合"));
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
#[cfg(feature = "gui")]
fn beam_eccentricity_edit_and_saved_reload_reject_results_from_previous_connection() {
    use result_validity::ResultInputKey;
    let mut app = calculated();
    let key = ResultInputKey::Static(StaticCaseKey::User(LoadCaseId(0)));
    app.require_result_input(key.clone()).unwrap();
    let original = app.result_input(&key);
    assert!(
        app.apply_model_edit(Box::new(sepika_edit::SetBeamVerticalEccentricity {
            member: sepika_core::model::SupportMemberId::Primary(ElemId(2)),
            value: Some(100.0)
        }))
    );
    assert_ne!(app.result_input(&key), original);
    assert!(app.require_result_input(key.clone()).is_err());
    let (mut loaded, path) = restore(
        &app,
        Some(ANALYSIS_CONTRACT_VERSION),
        Some(DESIGN_CONTRACT_VERSION),
        "734-eccentric-old-input",
    );
    assert_eq!(
        loaded.core.model.elements[2].vertical_eccentricity,
        Some(100.0)
    );
    assert!(loaded.require_result_input(key.clone()).is_err());
    loaded.run_linear_static(LoadCaseId(0));
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
    loaded.require_result_input(key.clone()).unwrap();
    app.undo_action();
    assert_eq!(app.result_input(&key), original);
    assert!(app.core.scoped.staleness.results_stale);
    std::fs::remove_file(path).unwrap();
}
