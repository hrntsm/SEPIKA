use super::{draw_th_detail_content_at, steel_column_lateral_buckling_input, App};
use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::model::{EndCondition, MassMethod, Model};
use sepika_design_jp::member_design_check::{run_member_design_checks, MemberDesignCheckOptions};
use sepika_design_jp::{CheckKind, CheckOutcome, LoadTerm};
use sepika_element::frame::beam::BeamElement;
use sepika_solver::dynamic::timehistory::GroundMotion;

#[test]
fn h_column_direct_c_still_changes_static_checks() {
    let forces = steel_column_lateral_buckling_input::moments(215e6);
    for (c_direct, expected_fb, expected_ratio, expected_ok) in [
        (None, "fbX=192.3854 N/mm²", 0.973_267_776_f64, true),
        (Some(0.8), "fbX=182.8236 N/mm²", 1.024_170_496_f64, false),
    ] {
        let mut model = steel_column_lateral_buckling_input::column(0.0, true);
        model.steel_design_attrs[0].c_direct = c_direct;
        let id = model.elements[0].id;
        let report = run_member_design_checks(
            &model,
            &[(id, forces.clone())],
            &[],
            &MemberDesignCheckOptions {
                term: LoadTerm::Short,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(report.member_checks.len(), 3);
        for (_, _, outcome) in report.member_checks {
            let CheckOutcome::Checked(result) = outcome else {
                panic!("{outcome:?}");
            };
            let bending = result
                .components
                .iter()
                .find(|component| component.kind == CheckKind::AxialBending)
                .unwrap();
            assert!(
                result.detail.contains("横座屈長さ lb=3000 mm"),
                "{result:?}"
            );
            assert!(bending.detail.contains(expected_fb), "{bending:?}");
            assert!((bending.ratio - expected_ratio).abs() < 1e-8, "{bending:?}");
            assert_eq!(result.ok(), expected_ok);
        }
    }
}

fn detail_labels(app: &mut App, id: sepika_core::ElemId) -> String {
    let mut view = super::super::ViewportState::default();
    view.capture_result_sources(app);
    egui::Context::default()
        .run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 1200.0),
                )),
                ..Default::default()
            },
            |ui| draw_th_detail_content_at(ui, app, id, 0, &view),
        )
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn linear_column(mz_nmm: f64) -> (Model, sepika_job::AnalysisSettings, GroundMotion) {
    let mut model = steel_column_lateral_buckling_input::column(0.0, false);
    model.elements[0].end_cond[1] = EndCondition::Pinned;
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.nodes[1].restraint.set_free(Dof::Ux);
    model.nodes[1].mass = Some([20.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    model.mass_method = MassMethod::LumpedOnly;
    model.steel_design_attrs[0].column_lateral_buckling_end_supports =
        model.column_end_supports(&model.elements[0]);
    model.steel_design_attrs[0].c_direct = Some(2.3);
    let k = BeamElement::new(&model.elements[0], &model).local_stiffness();
    let dt = 0.01_f64;
    let u_mm = mz_nmm / k.get(5, 7).abs();
    let wave = GroundMotion {
        dt,
        accel_x: vec![
            0.0,
            -(4.0 / dt.powi(2) + k.get(7, 7) / 20.0) * u_mm,
            16.0 * u_mm / dt.powi(2),
        ],
        accel_y: None,
        accel_theta: None,
    };
    let cfg = sepika_job::AnalysisSettings {
        mass_method: MassMethod::LumpedOnly,
        th_damping: 0.0,
        th_record_every: 2,
        ..Default::default()
    };
    (model, cfg, wave)
}

#[test]
fn h_column_bending_check_shows_actual_coefficient_and_unstored_step_after_save() {
    let (model, cfg, wave) = linear_column(259e6);
    let checkpoint =
        sepika_job::time_history_restart::compute_checkpoint(model.clone(), cfg, wave.clone(), 1)
            .unwrap();
    let mut prior = serde_json::to_value(checkpoint).unwrap();
    prior["result"]["recording"]
        .as_object_mut()
        .unwrap()
        .remove("symmetric_column_bending");
    let old = serde_json::from_value(prior).unwrap();
    let error =
        sepika_job::time_history_restart::resume_checkpoint(model.clone(), cfg, wave.clone(), old)
            .unwrap_err()
            .to_string();
    assert!(
        error.contains("全確定時刻の横座屈照査がありません。先頭から再計算"),
        "{error}"
    );

    let id = model.elements[0].id;
    let mut app = App::default();
    assert!(app.load_model(model));
    app.generate_stories_action();
    app.core.analysis_cfg = cfg;
    app.run_time_history(wave);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );

    let path = std::env::temp_dir().join(format!(
        "symmetric-steel-history-{}.ovika",
        uuid::Uuid::new_v4()
    ));
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut restored = App::default();
    restored.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();

    let before = detail_labels(&mut app, id);
    let text = detail_labels(&mut restored, id);
    assert_eq!(text, before);
    for expected in [
        "分布と照査条件を確認できた確定時刻 3/3",
        "補剛区間 ξ=0–1",
        "lb=3000 mm",
        "強軸曲げ検定比 1.00772 (NG)",
        "支配時刻 t=0.010000 s",
        "区間で単調変化",
        "C=1.750",
        "短期fb=223.83312 N/mm²",
        "通常検定用のC直接入力は、対象の時刻と補剛区間での有効性を確認できないため使用しません",
    ] {
        assert!(text.contains(expected), "{expected}: {text}");
    }
    assert!(!text.contains("C=2.300"), "{text}");
}

#[test]
fn past_case_does_not_approve_the_current_columns_changed_bracing() {
    let (model, cfg, mut wave) = linear_column(200e6);
    let id = model.elements[0].id;
    let mut app = App::default();
    assert!(app.load_model(model));
    app.generate_stories_action();
    app.core.analysis_cfg = cfg;
    app.run_time_history(wave.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let first = app.core.scoped.view_vibration_case.unwrap();
    let text = detail_labels(&mut app, id);
    assert!(
        text.lines()
            .any(|row| row.starts_with("強軸曲げ検定比") && row.contains("(OK)")),
        "{text}"
    );

    let attr = &mut app.core.model.steel_design_attrs[0];
    attr.column_lateral_buckling_end_supports = None;
    attr.lateral_brace_count = Some(1);
    wave.accel_x[2] += 10.0;
    app.run_time_history(wave);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert_ne!(app.core.scoped.view_vibration_case, Some(first));
    let text = detail_labels(&mut app, id);
    assert!(text.contains("中間横補剛が入力されています"), "{text}");
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .time_history_for(first)
        .is_some());
    app.core.scoped.view_vibration_case = Some(first);

    let stale = detail_labels(&mut app, id);
    assert!(
        stale.contains("選択中の時刻歴結果の生成入力と現在のモデルの一致を確認できません"),
        "{stale}"
    );
    assert!(
        !stale.contains("強軸曲げ検定比") && !stale.contains("C=1.750"),
        "{stale}"
    );
}

#[test]
fn material_only_failure_is_shown_alongside_the_unavailable_lateral_check() {
    use sepika_solver::dynamic::timehistory::{
        ColumnBendingMaterialFailure, SymmetricColumnBendingHistory, ThRecording,
    };

    let original = ThRecording {
        symmetric_column_bending: [(
            0,
            SymmetricColumnBendingHistory {
                committed_count: 3,
                evaluated_count: 0,
                lb_mm: Some(3000.0),
                material_failure: Some(ColumnBendingMaterialFailure {
                    step: 1,
                    time_s: 0.01,
                    x_mm: 1000.0,
                    mz_nmm: 280e6,
                    ratio: 1.03766,
                }),
                unavailable: [
                    "弱軸曲げと強軸曲げが同時に作用するため、強軸曲げの横座屈は未算定です".into(),
                ]
                .into_iter()
                .collect(),
                ..Default::default()
            },
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let saved = rmp_serde::to_vec_named(&original).unwrap();
    let restored: ThRecording = rmp_serde::from_slice(&saved).unwrap();
    assert_eq!(
        restored.symmetric_column_bending,
        original.symmetric_column_bending
    );
    let output = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
        super::draw_symmetric_column_bending(ui, &restored, 0, 3, false, || true);
    });
    let labels: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        labels
            .iter()
            .any(|row| row.contains("未算定：弱軸曲げと強軸曲げ")),
        "{labels:?}"
    );
    assert!(
        labels
            .iter()
            .any(|row| row.contains("独立NG")
                && row.contains("Mz=280.000 kN·m（i端から 1000.000 mm）")),
        "{labels:?}"
    );
    assert!(!labels.iter().any(|row| row.contains("(OK)")), "{labels:?}");
}
