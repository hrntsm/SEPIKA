use super::*;
use sepika_core::ids::MaterialId;
use sepika_core::model::{
    FrameSectionUse, LoadCase, LoadCaseKind, LoadCombination, MaterialCategory, NodalLoad,
};
use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};
use sepika_design_jp::ultimate::UltimateDemandMode;
use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;

fn column(cft: bool) -> App {
    let mut model = crate::sample::portal_frame();
    model
        .nodes
        .retain(|n| n.id == NodeId(0) || n.id == NodeId(2));
    model.nodes[1].id = NodeId(1);
    model.nodes[1].coord = [0.0, 0.0, 3000.0];
    model.elements.truncate(1);
    model.elements[0].nodes = [NodeId(0), NodeId(1)].into_iter().collect();
    model.elements[0].local_axis.ref_vector = [1.0, 0.0, 0.0];
    let shape = if cft {
        SectionShape::CftBox {
            height: 400.0,
            width: 400.0,
            thick: 12.0,
            corner_r: Some(0.0),
        }
    } else {
        SectionShape::RcColumnRect {
            b: 400.0,
            d: 600.0,
            rebar: RcRectColumnRebar {
                main_dia: 22.0,
                x: vec![3],
                y: vec![2],
                cover: 40.0,
                hoop: RectColumnHoop {
                    dia: 10.0,
                    pitch: 150.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
        }
    };
    let mut section = shape.to_section(SectionId(0), "実対象柱".into());
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = Some(MaterialId(0));
    section.steel_material = cft.then_some(MaterialId(1));
    section.rebar_material = (!cft).then_some(MaterialId(1));
    section.shear_rebar_material = (!cft).then_some(MaterialId(1));
    model.sections = vec![section];
    let mut steel = model.materials[0].clone();
    steel.id = MaterialId(1);
    steel.category = if cft {
        MaterialCategory::Steel
    } else {
        MaterialCategory::Rebar
    };
    steel.name = if cft { "SN400B" } else { "SD295A" }.into();
    steel.fy = Some(if cft { 235.0 } else { 295.0 });
    model.materials[0].category = MaterialCategory::Concrete;
    model.materials[0].name = "Fc24".into();
    model.materials[0].young = 25000.0;
    model.materials[0].density = 2.4e-9;
    model.materials[0].fc = Some(24.0);
    model.materials[0].fy = None;
    model.materials.push(steel);
    model.load_cases = [
        (LoadCaseKind::Dead, 10000.0, -100000.0),
        (LoadCaseKind::Dead, -3000.0, -20000.0),
        (LoadCaseKind::Live, 5000.0, -30000.0),
        (LoadCaseKind::LiveSeismic, 1000.0, -10000.0),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (kind, x, z))| LoadCase {
        id: LoadCaseId(i as u32),
        name: format!("任意名{i}"),
        kind,
        nodal: vec![NodalLoad::manual(NodeId(1), [x, 0.0, z, 0.0, 0.0, 0.0])],
        member: vec![],
    })
    .collect();
    model.combinations = vec![LoadCombination {
        name: "保存架構用".into(),
        terms: vec![
            (LoadCaseId(0), 1.0),
            (LoadCaseId(1), 1.0),
            (LoadCaseId(2), 1.0),
        ],
    }];
    model.ultimate_gravity_combination = Some(0);
    let mut app = App::default();
    app.load_model(model);
    app.core.analysis_cfg.threads = 1;
    app.core.ultimate_upper_factor = 1.0;
    app
}

fn solve_statics(app: &mut App) {
    for i in 0..4 {
        app.run_linear_static(LoadCaseId(i));
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
    }
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-6 * expected.abs().max(1.0),
        "actual={actual} expected={expected}"
    );
}

fn check_app(app: &mut App, cft: bool, expected_n: f64, total_q: Option<f64>) {
    if cft {
        let checks = app.compute_cft_ultimate_checks().unwrap();
        assert_eq!(checks.len(), 1);
        let c = &checks[0];
        assert_eq!(c.elem, ElemId(0));
        close(c.n_design, expected_n);
        close(
            c.axial_margin,
            if expected_n > 0.0 {
                c.ncu / expected_n
            } else {
                c.ntu / -expected_n
            },
        );
        println!(
            "CFT App observed N={} Ncu={} Ntu={} margin={} totalQ={total_q:?}",
            c.n_design, c.ncu, c.ntu, c.axial_margin
        );
    } else {
        let checks = app.compute_ultimate_checks().unwrap();
        assert_eq!(checks.len(), 1);
        let c = &checks[0];
        assert_eq!(c.elem, ElemId(0));
        assert!(c.qsu.is_finite() && c.qsu > 12000.0 && c.qmu > 0.0);
        if let Some(q) = total_q {
            close(c.qmu, q.abs());
            close(c.shear_margin, c.qsu / q.abs());
            assert!((c.shear_margin - (c.qsu - 12000.0) / q.abs()).abs() > 1e-4);
        } else {
            close(c.shear_margin, (c.qsu - 12000.0) / c.qmu);
        }
        println!(
            "RC App observed N={expected_n} Qsu={} Qmu={} margin={} detail={}",
            c.qsu, c.qmu, c.shear_margin, c.detail
        );
    }
}

fn app_error(app: &mut App, cft: bool) -> String {
    let error = if cft {
        app.compute_cft_ultimate_checks().unwrap_err()
    } else {
        app.compute_ultimate_checks().unwrap_err()
    };
    println!("App rejected cft={cft}: {error}");
    error
}

#[test]
fn actual_rc_cft_static_frame_terms_reach_app_checks_with_signed_equilibrium() {
    for cft in [false, true] {
        let mut app = column(cft);
        solve_statics(&mut app);
        for (case, expected_n, expected_q) in [
            (0, -100000.0, -10000.0),
            (1, -20000.0, 3000.0),
            (2, -30000.0, -5000.0),
        ] {
            let forces = &app
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .statics
                .iter()
                .find(|(key, _)| *key == StaticCaseKey::User(LoadCaseId(case)))
                .unwrap()
                .1
                .member_forces[0]
                .1;
            close(forces.at[0].1[0], expected_n);
            close(forces.at[0].1[1], expected_q);
            println!("static raw cft={cft} case={case} at={:?}", forces.at);
        }
        let d = app.ultimate_demand_by_elem().unwrap()[0].1;
        println!("static cft={cft} local_x=globalZ local_y=globalX observed={d:?}");
        assert_eq!(d.mode, UltimateDemandMode::StaticHinge);
        close(d.n_axial, 150000.0);
        close(d.q_long.unwrap(), 12000.0);
        close(d.mz.abs(), 36000000.0);
        close(d.my, 0.0);
        check_app(&mut app, cft, 150000.0, None);
        let bundle = app.core.scoped.results.as_mut().unwrap();
        bundle
            .statics
            .iter_mut()
            .find(|(k, _)| *k == StaticCaseKey::User(LoadCaseId(1)))
            .unwrap()
            .1
            .member_forces
            .clear();
        let error = app_error(&mut app, cft);
        for expected in [
            "gravity_member_missing:",
            "case=1",
            "member=0",
            "purpose=frame_long",
            "mode=static_hinge",
        ] {
            assert!(error.contains(expected), "missing {expected}: {error}");
        }
    }
}

#[test]
fn actual_rc_cft_confirmed_run_reaches_app_checks_without_ql_addition_or_fallback() {
    for cft in [false, true] {
        let mut app = column(cft);
        solve_statics(&mut app);
        app.generate_stories_action();
        app.run_seismic(SeismicDir::X);
        app.core.analysis_cfg.push_steps = 3;
        app.run_pushover();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        app.core.ultimate_use_pushover = true;
        assert!(app_error(&mut app, cft).contains("step"));
        let bundle = app.core.scoped.results.as_mut().unwrap();
        let po = bundle.pushover.as_mut().unwrap();
        let history = po.confirmed_history.as_ref().unwrap();
        assert!(history.len() >= 3);
        let response = history[1].clone();
        assert_ne!(response.step, history.last().unwrap().step);
        assert!(
            (response.members[0].shear_strong - history.last().unwrap().members[0].shear_strong)
                .abs()
                > 1.0
        );
        let lambda = po.steps[response.step as usize].load_factor;
        assert!(lambda > 0.0);
        println!(
            "confirmed cft={cft} selected_step={} last_step={} lambda={} cuts={:?}",
            response.step,
            history.last().unwrap().step,
            lambda,
            response.cuts
        );
        po.capacity_evaluation = Some(
            po.evaluation_point(
                EvaluationPurpose::HoldingCapacity,
                SeismicDir::X,
                response.step,
                "水平載荷済み非最終stepを採用".into(),
            )
            .unwrap(),
        );
        let run = po.wall_run.as_ref().unwrap();
        assert!(run.gravity_applied);
        assert_eq!(
            run.gravity_terms,
            vec![
                (LoadCaseId(0), 1.0),
                (LoadCaseId(1), 1.0),
                (LoadCaseId(2), 1.0)
            ]
        );
        bundle.pushover_x = Some(po.clone());
        let r = response
            .members
            .iter()
            .find(|r| r.elem == ElemId(0))
            .unwrap();
        let d = app.ultimate_demand_by_elem().unwrap()[0].1;
        println!(
            "confirmed cft={cft} run={} step={} observed={d:?} raw={r:?}",
            response.run_id, response.step
        );
        assert_eq!(d.mode, UltimateDemandMode::ConfirmedTotal);
        assert_eq!(d.q_long, None);
        close(d.n_axial, r.axial);
        close(d.n_axial, 150000.0);
        close(d.mz, r.m_strong);
        close(d.my, r.m_weak);
        close(d.shear.unwrap(), r.shear_strong);
        assert!(d.shear.unwrap() > 12000.0);
        close(
            d.shear.unwrap(),
            12000.0 + lambda * response.cuts[0].reference_n,
        );
        close(d.shear_weak.unwrap(), r.shear_weak);
        close(d.rp.unwrap(), r.rp);
        check_app(&mut app, cft, 150000.0, d.shear);
        let accepted = app.core.scoped.results.clone().unwrap();
        app.core.ultimate_upper_factor = 1.1;
        assert!(app_error(&mut app, cft).contains("倍率"));
        app.core.ultimate_upper_factor = 1.0;
        app.core.scoped.pushover_view_dir = SeismicDir::Y;
        app_error(&mut app, cft);
        app.core.scoped.pushover_view_dir = SeismicDir::X;
        for corruption in 0..7 {
            app.core.scoped.results = Some(accepted.clone());
            let po = app
                .core
                .scoped
                .results
                .as_mut()
                .unwrap()
                .pushover_x
                .as_mut()
                .unwrap();
            match corruption {
                0 => po
                    .capacity_evaluation
                    .as_mut()
                    .unwrap()
                    .input_generation
                    .clear(),
                1 => po.capacity_evaluation.as_mut().unwrap().direction = SeismicDir::Y,
                2 => po.capacity_evaluation.as_mut().unwrap().step = u32::MAX,
                3 => po.wall_run.as_mut().unwrap().gravity_applied = false,
                4 => po.wall_run.as_mut().unwrap().gravity_terms[0].1 = 0.5,
                5 => po.confirmed_history.as_mut().unwrap()[1].members.clear(),
                _ => po.confirmed_history.as_mut().unwrap()[1].members[0].axial = f64::NAN,
            }
            println!("confirmed corruption={corruption}");
            let error = app_error(&mut app, cft);
            let (cause, member) = match corruption {
                0 => ("confirmed_generation_mismatch:", "member=unresolved"),
                1 => ("ultimate_direction_mismatch:", "member=unresolved"),
                2 => ("confirmed_step_missing:", "member=unresolved"),
                3 => ("ultimate_gravity_missing:", "member=unresolved"),
                4 => ("ultimate_gravity_stale:", "member=unresolved"),
                5 => ("ultimate_response_missing:", "member=0"),
                _ => ("confirmed_member_nonfinite:", "member=0"),
            };
            for expected in [
                cause,
                member,
                "mode=confirmed_total",
                "purpose=frame_long",
                "evaluation_purpose=HoldingCapacity",
                "direction=X",
                "gravity_terms=[case=0 coefficient=",
            ] {
                assert!(
                    error.contains(expected),
                    "corruption={corruption} missing {expected}: {error}"
                );
            }
            for expected in [
                format!("run={}", response.run_id),
                format!(
                    "input_generation={}",
                    crate::summary::input_generation_hash(
                        response.input_generation.as_ref().unwrap()
                    )
                ),
                format!(
                    "step={}",
                    if corruption == 2 {
                        u32::MAX
                    } else {
                        response.step
                    }
                ),
                format!(
                    "selected_direction={}",
                    if corruption == 1 { "Y" } else { "X" }
                ),
            ] {
                assert!(
                    error.contains(&expected),
                    "corruption={corruption} missing {expected}: {error}"
                );
            }
            if corruption == 6 {
                assert!(error.contains("確定部材応答が非有限です"));
            }
            assert!(error.len() < 2000, "diagnostic length={}", error.len());
            for field in ["input_generation=", "selected_generation="] {
                assert_eq!(
                    error
                        .split_whitespace()
                        .find_map(|word| word.strip_prefix(field))
                        .unwrap()
                        .len(),
                    64
                );
            }
        }
        app.core.scoped.results = Some(accepted.clone());
        let old_run = accepted.pushover_x.as_ref().unwrap().clone();
        let old_point = old_run.capacity_evaluation.as_ref().unwrap();
        assert!(old_run
            .confirmed_response(old_point, EvaluationPurpose::HoldingCapacity)
            .is_ok());
        app.core.analysis_cfg.push_steps = 4;
        app.run_pushover();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let current_generation = app.result_input_with_settings(
            &ResultInputKey::Pushover(SeismicDir::X),
            app.core.analysis_cfg,
        );
        assert_ne!(old_point.input_generation, current_generation);
        let bundle = app.core.scoped.results.as_mut().unwrap();
        let current_records = bundle.input_records.clone();
        assert!(current_records.iter().any(|(key, bytes)| *key
            == ResultInputKey::Pushover(SeismicDir::X)
            && bytes == &current_generation));
        bundle.pushover_x = Some(old_run.clone());
        bundle.pushover = Some(old_run);
        assert_eq!(bundle.input_records, current_records);
        assert!(app
            .require_result_input(ResultInputKey::Pushover(SeismicDir::X))
            .is_ok());
        let error = app_error(&mut app, cft);
        for expected in [
            "ultimate_generation_stale:",
            "mode=confirmed_total",
            "purpose=frame_long",
            "member=unresolved",
            "増分解析を再実行",
            "採用stepを明示",
        ] {
            assert!(error.contains(expected), "missing {expected}: {error}");
        }
        assert!(error.len() < 2000, "diagnostic length={}", error.len());
        app.run_pushover();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        assert!(app_error(&mut app, cft).starts_with("ultimate_step_missing:"));
        let bundle = app.core.scoped.results.as_mut().unwrap();
        let po = bundle.pushover_x.as_mut().unwrap();
        let step = po.confirmed_history.as_ref().unwrap()[1].step;
        po.capacity_evaluation = Some(
            po.evaluation_point(
                EvaluationPurpose::HoldingCapacity,
                SeismicDir::X,
                step,
                "現在条件で再解析したstepを明示採用".into(),
            )
            .unwrap(),
        );
        bundle.pushover = Some(po.clone());
        let demand = app.ultimate_demand_by_elem().unwrap()[0].1;
        assert_eq!(demand.mode, UltimateDemandMode::ConfirmedTotal);
        check_app(&mut app, cft, 150000.0, demand.shear);
    }
}

#[test]
fn actual_rc_cft_saved_static_results_reject_changed_terms_and_keep_tension_sign() {
    for cft in [false, true] {
        let mut app = column(cft);
        solve_statics(&mut app);
        let path = std::env::temp_dir().join(format!(
            "sepika565-rc-cft-{cft}-{}.ovika",
            std::process::id()
        ));
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        std::fs::remove_file(path).unwrap();
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "{:?}",
            loaded.core.scoped.last_error
        );
        check_app(&mut loaded, cft, 150000.0, None);
        loaded.core.model.combinations[0].terms[1].1 = 0.0;
        app_error(&mut loaded, cft);
        let mut tension = column(cft);
        for case in &mut tension.core.model.load_cases {
            case.nodal[0].values[2] *= -1.0;
        }
        solve_statics(&mut tension);
        close(
            tension.ultimate_demand_by_elem().unwrap()[0].1.n_axial,
            -150000.0,
        );
        check_app(&mut tension, cft, -150000.0, None);
    }
}
