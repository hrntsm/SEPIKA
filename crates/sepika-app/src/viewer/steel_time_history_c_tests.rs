use super::{draw_th_detail_content_at, steel_column_lateral_buckling_input, App, ThRecording};
use sepika_design_jp::member_design_check::{run_member_design_checks, MemberDesignCheckOptions};
use sepika_design_jp::{CheckKind, CheckOutcome, LoadTerm};
use sepika_solver::dynamic::timehistory::{AxialForcePeaks, ResponseResult};

#[test]
fn h_column_direct_c_changes_static_but_not_time_history_check() {
    let forces = steel_column_lateral_buckling_input::moments(215e6);
    let times = vec![0.0, 1.0];
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

        let mut app = App::default();
        assert!(app.load_model(model));
        app.core.scoped.results = Some(crate::app::ResultsBundle {
            time_history: Some(ResponseResult {
                wall_capacities: Vec::new(),
                rc_wall_shear_method: Default::default(),
                time: times.clone(),
                peak_disp: Vec::new(),
                story_drift_angle: Vec::new(),
                history: Default::default(),
                recording: Some(ThRecording {
                    frame_time: times.clone(),
                    node_disp: vec![vec![[0.0; 6]; 2]; 2],
                    member_forces: vec![vec![Some(forces.clone())]; 2],
                    peak_member_forces: vec![Some(forces.clone())],
                    axial_force_peaks: vec![AxialForcePeaks::default()],
                    ..Default::default()
                }),
                nonlinear: false,
                applied_long_term: false,
                non_converged_steps: 0,
            }),
            ..Default::default()
        });
        let view = super::super::ViewportState::default();
        let output = egui::Context::default().run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 1200.0),
                )),
                ..Default::default()
            },
            |ui| draw_th_detail_content_at(ui, &mut app, id, 0, &view),
        );
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
                .any(|text| text.contains("採用横座屈長さ lb=3000 mm")),
            "{labels:?}"
        );
        for position in ["ξ=0.00:", "ξ=0.50:", "ξ=1.00:"] {
            assert!(
                labels
                    .windows(2)
                    .any(|pair| pair == [position, "検定比 0.973 (OK)"]),
                "{labels:?}"
            );
        }
    }
}
