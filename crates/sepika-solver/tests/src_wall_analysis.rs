#[path = "support/rc_wall_side_column_model.rs"]
mod rc_wall_side_column_model;
#[path = "support/src_wall_model.rs"]
mod src_wall_model;

use sepika_core::dof::DofMap;
use sepika_core::ids::{ElemId, LoadCaseId, NodeId};
use sepika_core::model::{AnalysisKind, HysteresisModel, LoadCaseKind, Model, NodalLoad};
use sepika_core::wall_expand::expand_wall_elements;
use sepika_element::wall::shear_section::wall_shear_rigidity;
use sepika_element::wall::src_face::SrcWallFaces;
use sepika_element::{
    behavior::{Ctx, LocalVec},
    factory::{build_nonlinear_behavior, resolve_wall_shear_hysteresis, StrengthBasis},
};
use sepika_solver::common::constraint::Reducer;
use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;
use sepika_solver::nonlinear::pushover::{
    pushover_analysis_recording_signed, PushoverControl, PushoverSign, PushoverTarget,
};
use sepika_solver::statics::analysis::{Analysis, SeismicDir};

fn close(actual: f64, expected: f64, relative_tolerance: f64) {
    assert!(
        (actual - expected).abs() <= relative_tolerance * expected.abs().max(1.0),
        "actual={actual} expected={expected}"
    );
}

#[test]
fn composite_plate_stiffness_matches_material_strip_energy_and_clear_panel_flexure() {
    for (embedded, ga_n, lateral_stiffness_n_mm) in [
        (false, 7_555_031_367.719_596, 1_792_352.015_273_499_3),
        (true, 10_821_234_419.819_07, 2_606_516.074_526_503_7),
    ] {
        use sepika_core::dof::{Dof, Dof6Mask};
        let (mut model, _, _) = expand_wall_elements(&src_wall_model::input_model(embedded));
        for n in &mut model.nodes[2..] {
            n.restraint = Dof6Mask::FIXED;
            n.restraint.set_free(Dof::Ux);
        }
        model.load_cases[0].kind = LoadCaseKind::Seismic;
        model.load_cases[0].nodal = (2..=3)
            .map(|i| NodalLoad::manual(NodeId(i), [50_000.0, 0.0, 0.0, 0.0, 0.0, 0.0]))
            .collect();
        let wall = model.element(ElemId(4)).unwrap();
        // 対称断面の片側6区間でせん断ひずみエネルギーの4次式を解析積分した値。
        close(wall_shear_rigidity(wall, &model).unwrap(), ga_n, 1e-8);
        let result = Analysis::prepare(&model)
            .unwrap()
            .linear_static(LoadCaseId(0))
            .unwrap();
        // 上下回転固定、中央壁柱のTimoshenko曲げ・せん断直列ばね。
        for node in [2, 3] {
            close(
                result.disp[node][0],
                100_000.0 / lateral_stiffness_n_mm,
                1e-8,
            );
        }
        let forces = result
            .member_forces
            .iter()
            .map(|(id, f)| (*id, f.at.as_slice()))
            .collect::<Vec<_>>();
        let [bottom, top] = SrcWallFaces::new(wall, &model)
            .unwrap()
            .recover(&forces)
            .unwrap();
        for end in [bottom, top] {
            close(end.shear_n, -100_000.0, 1e-9);
            close(end.axial_n, 0.0, 1e-8);
            close(end.moment_nmm.abs(), 120_000_000.0, 1e-9);
        }
        close(top.moment_nmm - bottom.moment_nmm, -240_000_000.0, 1e-9);
    }
}

fn signed_run(
    model: &Model,
    sign: PushoverSign,
) -> sepika_solver::nonlinear::pushover::PushoverResult {
    let map = DofMap::build(model);
    let reducer = Reducer::build(model, &map);
    let mut run = pushover_analysis_recording_signed(
        model,
        &map,
        &reducer,
        SeismicDir::X,
        sign,
        6,
        PushoverTarget {
            max_disp: None,
            max_drift_angle: None,
        },
        PushoverControl::LoadOnly,
        true,
        Default::default(),
    )
    .unwrap();
    run.identify_wall_input(vec![12, 34]);
    run
}

#[test]
fn src_wall_factory_uses_takeda_and_directional_fixed_plateaus() {
    let (model, _, _) = expand_wall_elements(&src_wall_model::input_model(true));
    let wall = model.element(ElemId(4)).unwrap();
    assert_eq!(
        resolve_wall_shear_hysteresis(wall, &model, AnalysisKind::Incremental),
        HysteresisModel::Takeda,
    );
    let ctx = Ctx { model: &model };
    for (sign, expected_n) in [(1.0, 50_000.0), (-1.0, -35_000.0)] {
        let mut element = build_nonlinear_behavior(
            wall,
            &model,
            StrengthBasis::MaterialStrength,
            AnalysisKind::Incremental,
            Some([50_000.0, 35_000.0]),
        );
        let mut push = LocalVec {
            data: smallvec::smallvec![0.0; 24],
        };
        push.data[12] = sign * 0.2;
        push.data[18] = sign * 0.2;
        for _ in 0..3 {
            element.update_state(&push, true, &ctx).unwrap();
            let f = element.internal_force(&ctx);
            close(f.data[12] + f.data[18], expected_n, 1e-8);
        }
    }
}

#[test]
fn signed_src_runs_recover_same_step_wall_actions_and_gate_every_later_design_use() {
    let mut source = src_wall_model::analysis_input_model(true);
    for load in &mut source.load_cases[0].nodal {
        load.values[2] = -400_000.0;
    }
    let (model, _, _) = expand_wall_elements(&source);
    let positive = signed_run(&model, PushoverSign::Positive);
    let mut negative = signed_run(&model, PushoverSign::Negative);
    let step = 2;
    let p = positive
        .evaluation_point(EvaluationPurpose::Ds, SeismicDir::X, step, "照査".into())
        .unwrap();
    let n = negative
        .evaluation_point(EvaluationPurpose::Ds, SeismicDir::X, step, "照査".into())
        .unwrap();
    let p = positive
        .confirmed_response(&p, EvaluationPurpose::Ds)
        .unwrap();
    let n = negative
        .confirmed_response(&n, EvaluationPurpose::Ds)
        .unwrap();
    assert_eq!(p.sign, PushoverSign::Positive);
    assert_eq!(n.sign, PushoverSign::Negative);
    assert_ne!(p.run_id, n.run_id);
    assert!(positive.capacity_curve[step as usize].roof_disp > 0.0);
    assert!(negative.capacity_curve[step as usize].roof_disp < 0.0);
    let [pb, pt] = p.src_walls[0].ends.unwrap();
    let [nb, nt] = n.src_walls[0].ends.unwrap();
    assert_eq!(p.src_frame_faces.len(), 4);
    for (end, wall) in [pb, pt].into_iter().enumerate() {
        let compression_n: f64 = p
            .src_frame_faces
            .iter()
            .filter(|record| matches!(record.elem, ElemId(0) | ElemId(1)))
            .map(|column| -column.faces[end].as_ref().unwrap()[0])
            .sum();
        close(compression_n, wall.columns_compression_n, 1e-9);
    }
    let load_n = positive.steps[step as usize].load_factor * 200_000.0;
    close(p.src_walls[0].demand_n.unwrap(), load_n, 1e-8);
    let shear_span_ratio = pb.moment_nmm.abs().max(pt.moment_nmm.abs()) / (load_n * 2800.0);
    assert!((1.0..=3.0).contains(&shear_span_ratio));
    close(pt.moment_nmm - pb.moment_nmm, -load_n * 6400.0, 1e-8);
    // 式(38.2)の手計算。壁の全せい2800 mm・有効せい2500 mm・側柱主筋10本とH形鋼。
    let column_compression_n = pb.columns_compression_n.min(pt.columns_compression_n);
    let qu_n = 1_959_133.076_538_202_8 / (shear_span_ratio + 0.12).sqrt()
        + 2_581_063.880_115_630_6
        + 0.0625 * column_compression_n;
    close(p.src_walls[0].limit_n.unwrap(), qu_n, 1e-9);
    for (p, n) in [(pb, nb), (pt, nt)] {
        close(p.axial_n, 800_000.0, 1e-8);
        close(n.axial_n, p.axial_n, 1e-8);
        close(n.shear_n, -p.shear_n, 1e-8);
        close(n.moment_nmm, -p.moment_nmm, 1e-8);
    }
    let failing = &mut negative.confirmed_history.as_mut().unwrap()[step as usize].src_walls[0];
    failing.limit_n = Some(0.9 * failing.demand_n.unwrap());
    for purpose in [EvaluationPurpose::Ds, EvaluationPurpose::HoldingCapacity] {
        assert!(negative
            .evaluation_point(purpose, SeismicDir::X, step - 1, "照査".into())
            .is_ok());
        for i in step..negative.steps.len() as u32 {
            let reason = negative
                .evaluation_point(purpose, SeismicDir::X, i, "照査".into())
                .unwrap_err();
            assert!(
                reason.contains("SRC耐震壁 ID 4 step 2") && reason.contains("超過"),
                "{reason}"
            );
        }
    }
    negative.confirmed_history.as_mut().unwrap()[step as usize]
        .src_walls
        .clear();
    let missing = negative.shear_design_unavailable_at(step + 1).unwrap();
    assert!(
        missing.contains("step 2") && missing.contains("照査がありません"),
        "{missing}"
    );
}

#[test]
fn gravity_only_step_preserves_bending_and_does_not_zero_small_real_shear() {
    let (model, _, _) = expand_wall_elements(&src_wall_model::analysis_input_model(true));
    let run = signed_run(&model, PushoverSign::Positive);
    let step = &run.confirmed_history.as_ref().unwrap()[0];
    let review = &step.src_walls[0];
    let [bottom, top] = review.ends.unwrap();
    assert!((bottom.moment_nmm.abs().max(top.moment_nmm.abs())) > 1.0e6);
    assert_eq!(review.demand_n, Some(0.0), "{review:?}");
    assert_eq!(review.limit_n, None);
    assert_eq!(run.shear_design_unavailable_at(0), None);

    let mut model = model;
    model.load_cases[0].nodal[0].values[0] = 0.01;
    let run = signed_run(&model, PushoverSign::Positive);
    let review = &run.confirmed_history.as_ref().unwrap()[0].src_walls[0];
    close(review.demand_n.unwrap(), 0.01, 1e-6);
    assert!(run
        .shear_design_unavailable_at(0)
        .unwrap()
        .contains("せん断スパン比"));
}

#[test]
fn src_girder_confirmation_recovers_the_loaded_actual_column_faces() {
    use sepika_core::model::{MemberLoad, MemberLoadKind};

    let mut source = src_wall_model::analysis_input_model(true);
    for load in &mut source.load_cases[0].nodal {
        load.values[2] = -400_000.0;
    }
    source.elements[2].rigid_zone.face_i = Some(17.0);
    source.elements[2].rigid_zone.face_j = Some(35.0);
    source.load_cases[0]
        .member
        .push(MemberLoad::full_length_uniform(
            ElemId(2),
            [0.0, 0.0, -1.0],
            2200.0,
            20.0,
        ));
    let recover = |source: &Model| {
        let (model, _, _) = expand_wall_elements(source);
        let run = signed_run(&model, PushoverSign::Positive);
        run.confirmed_history
            .unwrap()
            .remove(0)
            .src_frame_faces
            .into_iter()
            .find(|review| review.elem == ElemId(2))
            .unwrap()
            .faces
    };
    let [i, j] = recover(&source).map(Result::unwrap);
    close(j[1] - i[1], -32_000.0, 1e-9);
    close(j[5] - i[5], i[1] * 1600.0 - 25_600_000.0, 1e-8);

    source.load_cases[0].member.push(MemberLoad::manual(
        ElemId(2),
        [0.0, 0.0, -1.0],
        MemberLoadKind::Point { a: 300.0, p: 100.0 },
    ));
    let [i, j] = recover(&source);
    assert!(i.unwrap_err().contains("集中部材荷重"));
    assert!(j.is_ok());
}

#[test]
fn actual_src_wall_limit_can_fall_below_the_fixed_spring_before_run_ends() {
    let mut source = src_wall_model::analysis_input_model(true);
    for load in &mut source.load_cases[0].nodal {
        load.values[2] = -400_000.0;
    }
    source.stories[1].seismic_weight = Some(25_000_000.0);
    source.stories[1].weight_override = Some(25_000_000.0);
    let (model, _, _) = expand_wall_elements(&source);
    let map = DofMap::build(&model);
    let reducer = Reducer::build(&model, &map);
    let mut run = pushover_analysis_recording_signed(
        &model,
        &map,
        &reducer,
        SeismicDir::X,
        PushoverSign::Positive,
        64,
        PushoverTarget {
            max_disp: None,
            max_drift_angle: None,
        },
        PushoverControl::LoadOnly,
        true,
        Default::default(),
    )
    .unwrap();
    run.identify_wall_input(vec![12, 34]);
    let history = run.confirmed_history.as_ref().unwrap();
    let step = history
        .iter()
        .find(|step| {
            let wall = &step.src_walls[0];
            wall.demand_n
                .zip(wall.limit_n)
                .is_some_and(|(q_n, qu_n)| q_n > qu_n)
        })
        .expect("固定ばね耐力以下でも38条耐力を超えた確定ステップ");
    let wall = &step.src_walls[0];
    let (elem, fixed_n) = run.wall_capacities[0];
    let [demand_n, limit_n] = [wall.demand_n.unwrap(), wall.limit_n.unwrap()];
    assert_eq!(wall.elem, elem);
    assert!(demand_n > limit_n + 100.0);
    assert!(demand_n < fixed_n[0] - 100.0);
    assert!(step.step > 0 && history.last().unwrap().step > step.step);
    for purpose in [EvaluationPurpose::Ds, EvaluationPurpose::HoldingCapacity] {
        assert!(run
            .evaluation_point(purpose, SeismicDir::X, step.step - 1, "確認".into())
            .is_ok());
        for candidate in &history[step.step as usize..] {
            let reason = run
                .evaluation_point(purpose, SeismicDir::X, candidate.step, "確認".into())
                .unwrap_err();
            assert!(
                reason.contains(&format!("step {}（38条の終局せん断耐力を超過", step.step)),
                "{reason}"
            );
        }
    }
}
