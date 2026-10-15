use super::*;
use crate::app::signed_q0_acceptance_tests::fixture;

fn labels(app: &App, rec: &ThRecording) -> String {
    let elem = &app.core.model.elements[0];
    let axis = super::super::member_axis_endpoints(elem, &app.core.model).unwrap();
    egui::Context::default()
        .run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(3000.0, 4000.0),
                )),
                ..Default::default()
            },
            |ui| draw_peak_check(ui, app, elem, 0, rec, true, true, &axis),
        )
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn signed_q0_th_detail_missing_repaired_missing_does_not_render_ordinary_check() {
    let mut app = App::default();
    app.load_model(fixture::model());
    app.core.analysis_cfg.qd_method = sepika_design_jp::QdMethod::Qd1;
    app.run_combination(0);
    let peak = app.core.scoped.results.as_ref().unwrap().member_forces[0]
        .1
        .clone();
    app.run_combination(1);
    let rec = ThRecording {
        peak_member_forces: vec![Some(peak)],
        ..Default::default()
    };
    for valid in [false, true, false] {
        app.core.model.load_cases[0].member[0].dir =
            if valid { [0.0, 0.0, -1.0] } else { [0.0; 3] };
        let text = labels(&app, &rec);
        if valid {
            assert!(text.contains("検定比"), "{text}");
            assert!(!text.contains("検定不能"), "{text}");
        } else {
            assert!(text.contains("Q0"), "{text}");
            assert!(!text.contains("検定比"), "{text}");
        }
    }
    app.core.model.load_cases[0].member[0].dir = [0.0, 0.0, -1.0];
    let original = app.core.scoped.results.as_ref().unwrap().combos.clone();
    for valid in [false, true, false] {
        app.core.scoped.results.as_mut().unwrap().combos =
            if valid { original.clone() } else { vec![] };
        let text = labels(&app, &rec);
        if valid {
            assert!(text.contains("検定比"), "{text}");
        } else {
            assert!(text.contains("重力参照内力"), "{text}");
            assert!(!text.contains("検定比"), "{text}");
        }
    }
}

#[test]
fn signed_q0_th_detail_steel_still_checks_without_rc_gravity_input() {
    let mut app = App::default();
    app.load_model(crate::sample::portal_frame());
    app.run_linear_static(sepika_core::ids::LoadCaseId(0));
    let peak = app.core.scoped.results.as_ref().unwrap().member_forces[0]
        .1
        .clone();
    let rec = ThRecording {
        peak_member_forces: vec![Some(peak)],
        ..Default::default()
    };
    assert!(app.core.scoped.results.as_ref().unwrap().combos.is_empty());
    let text = labels(&app, &rec);
    assert!(text.contains("検定比"), "{text}");
    assert!(!text.contains("重力参照内力"), "{text}");
}
