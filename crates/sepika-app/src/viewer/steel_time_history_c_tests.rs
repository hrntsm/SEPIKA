use super::{draw_th_detail_content_at, steel_column_lateral_buckling_input, App};
use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::model::{EndCondition, MassMethod};
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
    let view = super::super::ViewportState::default();
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

#[test]
fn h_column_bending_check_shows_actual_coefficient_and_unstored_step_after_save() {
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
    let u_mm = 259e6 / k.get(5, 7).abs();
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
        th_damping: 0.0,
        th_record_every: 2,
        ..Default::default()
    };
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
    app.core.analysis_cfg.th_damping = 0.0;
    app.core.analysis_cfg.th_record_every = 2;
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

    let text = detail_labels(&mut restored, id);
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
