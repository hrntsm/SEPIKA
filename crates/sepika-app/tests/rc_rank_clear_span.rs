use sepika_app::app::App;
use sepika_core::ids::ElemId;

#[path = "support/holding_clear_span.rs"]
mod holding_clear_span;
use holding_clear_span::rc_rank_portal;
#[path = "../../sepika-solver/tests/support/rc_wall_side_column_model.rs"]
mod rc_fixture;

#[test]
fn rc_rank_and_ds_are_held_when_clear_span_is_unconfirmed() {
    for (target, circular, height_mm, beam_faces, expected) in [
        (
            ElemId(2),
            false,
            3000.0,
            Some((None, Some(0.0))),
            "i端のフェース距離が未算定",
        ),
        (
            ElemId(2),
            false,
            3000.0,
            Some((Some(0.0), None)),
            "j端のフェース距離が未算定",
        ),
        (
            ElemId(2),
            false,
            3000.0,
            Some((None, None)),
            "両端のフェース距離が未算定",
        ),
        (
            ElemId(2),
            false,
            3000.0,
            Some((Some(-1.0), Some(0.0))),
            "i端のフェース距離 -1 mm が有限な非負値ではありません",
        ),
        (
            ElemId(2),
            false,
            3000.0,
            Some((Some(0.0), Some(f64::INFINITY))),
            "j端のフェース距離 inf mm が有限な非負値ではありません",
        ),
        (ElemId(0), false, 600.0, None, "正の内法長さが成立しません"),
        (ElemId(0), true, 400.0, None, "正の内法長さが成立しません"),
    ] {
        let mut app = rc_rank_portal(
            rc_fixture::expanded_model(),
            circular,
            height_mm,
            beam_faces,
        );
        let section = if target == ElemId(0) { 0 } else { 1 };
        let member = format!("部材 ID {}・断面 S{section}（ID {section}）", target.0);
        let error = app.compute_holding_capacity().err().expect("保留");
        assert!(error.contains(&member), "{error}");
        assert!(error.contains(expected), "{error}");
        assert!(
            error.contains("自動部材種別・Ds・必要保有水平耐力・合否を保留します"),
            "{error}"
        );
        assert!(app.core.scoped.holding_capacity_source.is_none());

        #[cfg(feature = "gui")]
        {
            let output = egui::Context::default().run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(4000.0, 10000.0),
                    )),
                    ..Default::default()
                },
                |ui| sepika_app::design_view::design_table(ui, &mut app),
            );
            let visible = output.shapes.iter().any(
                |shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.text() == error),
            );
            assert!(visible, "{error}");
        }

        app.core.design_rank_auto = false;
        let error = app.compute_holding_capacity().err().expect("保留");
        assert!(
            error.contains("全体・部分・局部崩壊形は採用保留"),
            "{error}"
        );
    }
}

#[test]
fn mixed_fiber_frame_reports_eccentricity_and_keeps_holding_capacity_adoption_separate() {
    use sepika_app::summary::{build_report_csv, seismic_story_metrics};
    use sepika_core::model::ElementKind;
    use sepika_solver::statics::analysis::SeismicDir;

    let mut model = rc_fixture::expanded_model();
    model.elements[0].kind = ElementKind::Fiber;
    model.elements[0].rigid_zone.face_i = Some(0.0);
    model.elements[0].rigid_zone.face_j = Some(0.0);
    let mut app = rc_rank_portal(model, false, 3000.0, None);
    let metric = seismic_story_metrics(&app).remove(0);
    assert!(metric.re.unwrap().is_finite());
    assert!(metric.fes.unwrap().is_finite());
    app.core.design_rank_auto = false;
    let holding_reason = app.compute_holding_capacity().err().unwrap();
    assert!(
        holding_reason.contains("全体・部分・局部崩壊形は採用保留"),
        "{holding_reason}"
    );

    let (_, x) = app
        .core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .statics
        .iter_mut()
        .find(|(key, _)| *key == sepika_app::app::StaticCaseKey::Seismic(SeismicDir::X))
        .unwrap();
    x.member_forces.retain(|(id, _)| *id != ElemId(0));
    let metric = seismic_story_metrics(&app).remove(0);
    let reason = metric.re.unwrap_err();
    assert!(
        reason.contains("柱0") && reason.contains("始端の内力"),
        "{reason}"
    );
    assert_eq!(metric.fes.unwrap_err(), reason);
    assert!(metric.drift.unwrap().is_some());
    assert!(build_report_csv(&app).contains(&reason));
    let holding_reason = app.compute_holding_capacity().err().unwrap();
    assert!(holding_reason.contains(&reason), "{holding_reason}");
}
