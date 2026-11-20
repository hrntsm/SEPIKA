use super::*;
use sepika_core::model::SteelDesignAttr;
use sepika_design_jp::member_design_check::{run_member_design_checks, MemberDesignCheckOptions};
use sepika_design_jp::{CheckKind, SteelFbBasis};
use sepika_solver::dynamic::timehistory::{AxialForcePeaks, ResponseResult};

#[path = "../../../../crates/sepika-design-jp/tests/fixtures/steel_h_column_combined_model.rs"]
mod input;

#[test]
fn eccentric_h_column_same_constant_moment() {
    const MZ_NMM: f64 = 215_000_000.0;
    for (offset_mm, expected_ratio) in [(0.0, 0.973_509_006), (500.0, 1.039_363_327)] {
        let mut model = input::cantilever(0.0, MZ_NMM, 0.0);
        model.load_cases.clear();
        model.elements[0].end_offsets_mm[1][2] = offset_mm;
        let id = model.elements[0].id;
        model.steel_design_attrs.push(SteelDesignAttr {
            elem: id,
            joint_flange_loss: 0.0,
            joint_web_loss: 0.0,
            scallop_web_loss: 0.0,
            lb_direct: None,
            lateral_brace_count: None,
            lk_y_direct: Some(3500.0),
            lk_z_direct: Some(3500.0),
            c_direct: None,
        });
        let forces = MemberForces {
            at: [0.0, 0.5, 1.0]
                .map(|pos| (pos, [0.0, 0.0, 0.0, 0.0, 0.0, MZ_NMM]))
                .to_vec(),
        };
        let report = run_member_design_checks(
            &model,
            &[(id, forces.clone())],
            &[],
            &MemberDesignCheckOptions {
                term: LoadTerm::Short,
                steel_fb_basis: SteelFbBasis::Asd2019,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(model.member_length(&model.elements[0]), 3000.0 + offset_mm);
        println!("offset_mm={offset_mm} normal");
        assert_eq!(report.member_checks.len(), 3);
        let expected_ng = offset_mm != 0.0;
        for (reported_id, pos, outcome) in report.member_checks {
            assert_eq!(reported_id, id);
            let CheckOutcome::Checked(result) = outcome else {
                panic!("{outcome:?}")
            };
            let eval = crate::app::evaluate_check(&result);
            assert!(eval.complete);
            let status = if eval.known_ng.is_some() { "NG" } else { "OK" };
            assert_eq!(eval.known_ng.is_some(), expected_ng);
            let axial = result
                .components
                .iter()
                .find(|c| c.kind == CheckKind::AxialBending)
                .unwrap();
            println!(
                "  pos={pos} ratio={:.12} {status} {}",
                axial.ratio, axial.detail
            );
            assert!((axial.ratio - expected_ratio).abs() < 1e-9);
        }

        let mut app = App::default();
        assert!(app.load_model(model));
        let times = vec![0.0, 1.0];
        let recording = ThRecording {
            frame_time: times.clone(),
            node_disp: vec![vec![[0.0; 6]; 2]; 2],
            member_forces: vec![vec![Some(forces.clone())]; 2],
            peak_member_forces: vec![Some(forces)],
            axial_force_peaks: vec![AxialForcePeaks::default()],
            ..Default::default()
        };
        app.core.scoped.results = Some(crate::app::ResultsBundle {
            time_history: Some(ResponseResult {
                wall_capacities: Vec::new(),
                rc_wall_shear_method: Default::default(),
                time: times,
                peak_disp: Vec::new(),
                story_drift_angle: Vec::new(),
                history: Default::default(),
                recording: Some(recording),
                nonlinear: false,
                applied_long_term: false,
                non_converged_steps: 0,
            }),
            ..Default::default()
        });
        let view = super::super::ViewportState::default();
        let axis =
            super::super::member_axis_endpoints(&app.core.model.elements[0], &app.core.model)
                .unwrap();
        println!("offset_mm={offset_mm} history_axis_mm={}", axis.length);
        assert_eq!(axis.length, 3000.0);
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
        let expected_gui = "検定比 0.974 (OK)";
        assert!(
            labels.contains(&"採用座屈長さ [mm]　強軸 3500 ／ 弱軸 3500"),
            "{labels:?}"
        );
        for pos in ["ξ=0.00:", "ξ=0.50:", "ξ=1.00:"] {
            assert!(
                labels.windows(2).any(|row| row == [pos, expected_gui]),
                "{labels:?}"
            );
        }
        assert!(!labels.contains(&"検定比 1.039 (NG)"), "{labels:?}");
        println!("  history_labels={labels:?}");
    }
}
