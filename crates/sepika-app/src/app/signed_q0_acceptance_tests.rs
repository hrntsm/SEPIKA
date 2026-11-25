use super::*;
#[path = "../../../sepika-design-jp/tests/fixtures/signed_beam_gravity_model.rs"]
pub(crate) mod fixture;

fn app() -> App {
    let mut app = App::default();
    app.load_model(fixture::model());
    app.core.analysis_cfg.rc_damage_control = false;
    app.core.analysis_cfg.qd_method = sepika_design_jp::QdMethod::Qd1;
    app.core.analysis_cfg.threads = 1;
    app
}

fn shear_demand(app: &App) -> f64 {
    let check = app.current_design_results().expect("現役検定");
    let component = check
        .member_checks
        .iter()
        .flat_map(|m| &m.positions)
        .filter_map(|p| match &p.outcome {
            sepika_design_jp::CheckOutcome::Checked(c) => Some(c),
            _ => None,
        })
        .flat_map(|c| &c.components)
        .find(|c| c.kind == sepika_design_jp::CheckKind::Shear)
        .unwrap_or_else(|| panic!("せん断検定: {:?}", check.member_checks));
    let capacity: f64 = component
        .detail
        .strip_prefix("QA=")
        .unwrap()
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        component.ratio < 2.0,
        "表示QAの丸めによる需要誤差が0.1N未満になる範囲"
    );
    component.ratio * capacity
}

#[test]
fn signed_q0_gui_selected_combination_observes_independent_end_reactions() {
    let mut app = app();
    let dead = app.core.model.load_cases[0].id;
    app.core.model.combinations[0]
        .terms
        .splice(0..1, [(dead, f64::MAX), (dead, 1.0), (dead, -f64::MAX)]);
    // D22を片側3本、d=539mm。両端Muによる需要は2×0.9×at×345×d/6000。
    let mechanism_n =
        2.0 * 0.9 * (3.0 * std::f64::consts::PI * 22.0_f64.powi(2) / 4.0) * 345.0 * 539.0 / 6000.0;
    for merged in [false, true] {
        if merged {
            let live = app.core.model.load_cases[1].member.pop().unwrap();
            app.core.model.load_cases[0].member.push(live);
        }
        app.run_combination(0);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        // 表示QAは小数第1位までなので、復元需要にもその丸め幅を認める。
        assert!((shear_demand(&app) - (1000.0 + mechanism_n)).abs() < 0.1);
    }
}

#[test]
fn signed_q0_gui_nonstandard_live_factor_remains_reasoned_unchecked() {
    let mut app = app();
    app.core.model.combinations[0].terms[1].1 = 0.5;
    app.run_combination(0);
    let checks = &app.current_design_results().unwrap().member_checks;
    assert!(!checks.is_empty());
    assert!(checks.iter().flat_map(|m| &m.positions).all(|p| matches!(&p.outcome,sepika_design_jp::CheckOutcome::Skipped {reason} if reason.contains("係数"))));
}

#[test]
fn signed_q0_gui_invalid_direction_repaired_invalid_never_revives_saved_success() {
    let mut app = app();
    app.run_combination(0);
    assert!(app.core.scoped.last_error.is_none());
    assert!(shear_demand(&app) > 1000.0);
    assert!(crate::summary::build_report_csv(&app).contains(",OK"));
    for valid in [false, true, false] {
        app.core.model.load_cases[0].member[0].dir =
            if valid { [0.0, 0.0, -1.0] } else { [0.0; 3] };
        app.core.scoped.staleness.mark_edited();
        assert!(app.require_selected_static_input().is_err());
        assert!(app.current_design_results().is_none());
        assert!(!crate::summary::build_report_csv(&app).contains(",OK"));
        let before = rmp_serde::to_vec_named(&app.core.model).unwrap();
        app.run_combination(0);
        if valid {
            assert!(app.core.scoped.last_error.is_none());
            assert!(shear_demand(&app) > 1000.0);
        } else {
            let reason = app.core.scoped.last_error.as_deref().unwrap();
            for token in ["荷重ケース 0", "任意名称", "member[0]", "部材 0", "正規化"]
            {
                assert!(reason.contains(token), "{reason}");
            }
            assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), before);
            assert!(app.require_selected_static_input().is_err());
            assert!(app.current_static().is_none());
            assert!(app.current_design_results().is_none());
            assert!(!crate::summary::build_report_csv(&app).contains(",OK"));
        }
    }
}

#[test]
fn damage_control_uses_the_saved_seismic_combination_without_a_separate_gravity_result() {
    let mut app = app();
    app.core.analysis_cfg.rc_damage_control = true;
    app.core.model.combinations.truncate(1);
    app.run_combination(0);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    // 左端の支点反力は固定・積載の部材荷重各1 kNと先端の地震荷重10 kNの和。
    assert!((shear_demand(&app) - 12_000.0).abs() < 0.1);

    app.core.scoped.results.as_mut().unwrap().member_forces[0]
        .1
        .at[0]
        .1[1] = 0.0;
    app.run_design_check();
    let positions = &app.current_design_results().unwrap().member_checks[0].positions;
    assert!(!positions.is_empty());
    assert!(positions.iter().all(|position| {
        matches!(&position.outcome, sepika_design_jp::CheckOutcome::Skipped { reason } if reason.contains("保存応答が一致しません"))
    }));

    app.run_linear_static(LoadCaseId(2));
    app.run_design_check();
    let results = app.current_design_results().unwrap();
    let positions = &results.member_checks[0].positions;
    assert!(!positions.is_empty());
    assert!(positions.iter().all(|position| {
        matches!(&position.outcome, sepika_design_jp::CheckOutcome::Skipped { reason } if reason.contains("G+P"))
    }));
}

pub(crate) fn grouped_rc_qd1_model(split: bool) -> sepika_core::model::Model {
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, NodeId};
    use sepika_core::model::MemberLoad;

    let mut model = fixture::model();
    model.nodes[1].coord[0] = 4000.0;
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.load_cases[0].member = vec![MemberLoad::full_length_uniform(
        ElemId(0),
        [0.0, 0.0, -1.0],
        4000.0,
        10.0,
    )];
    model.load_cases[1].member.clear();
    if split {
        let mut middle = model.nodes[0].clone();
        middle.id = NodeId(2);
        middle.coord[0] = 2000.0;
        middle.restraint = Dof6Mask::FREE;
        model.nodes.push(middle);
        let mut right = model.elements[0].clone();
        right.id = ElemId(1);
        right.nodes[0] = NodeId(2);
        model.elements[0].nodes[1] = NodeId(2);
        model.elements.push(right);
        model.girder_groups = vec![vec![ElemId(0), ElemId(1)]];
        model.load_cases[0].member = [ElemId(0), ElemId(1)]
            .map(|id| MemberLoad::full_length_uniform(id, [0.0, 0.0, -1.0], 2000.0, 10.0))
            .into();
    }
    model
}

#[test]
fn grouped_rc_qd1_static_selected_combination_preserves_whole_span_demand() {
    let mut app = app();
    let my_nmm = 0.9 * (3.0 * std::f64::consts::PI * 22.0_f64.powi(2) / 4.0) * 345.0 * 539.0;
    let expected_n = 20_000.0 + 2.0 * my_nmm / 4000.0;
    for split in [false, true] {
        assert!(app.load_model(grouped_rc_qd1_model(split)));
        app.run_combination(0);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let results = app.current_design_results().unwrap();
        assert_eq!(results.member_checks.len(), if split { 2 } else { 1 });
        for member in &results.member_checks {
            let shear_components: Vec<_> = member
                .positions
                .iter()
                .filter_map(|pos| match &pos.outcome {
                    sepika_design_jp::CheckOutcome::Checked(result) => Some(&result.components),
                    _ => None,
                })
                .flatten()
                .filter(|c| c.kind == sepika_design_jp::CheckKind::Shear)
                .collect();
            assert!(!shear_components.is_empty(), "{:?}", member.positions);
            for shear in shear_components {
                let qa_n = shear
                    .detail
                    .strip_prefix("QA=")
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap();
                assert!((qa_n * shear.ratio - expected_n).abs() < 0.1, "{shear:?}");
            }
        }
    }
}
