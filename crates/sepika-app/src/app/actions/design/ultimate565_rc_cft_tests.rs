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
            horizontal_thick: 12.0,
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
        origin: Default::default(),
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
    if cft {
        model.steel_design_attrs = vec![
            crate::app::cft_column_buckling_tests::input::direct_lengths(
                ElemId(0),
                Some(3000.0),
                Some(3000.0),
            ),
        ];
    }
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
        assert_eq!(d.q_long_weak, Some(0.0));
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
fn static_rc_cft_reject_case_superposition_with_iterated_tension_only_brace() {
    use sepika_core::dof::{Dof, Dof6Mask};
    use sepika_core::model::{ElementKind, Section};

    for cft in [false, true] {
        let mut app = column(cft);
        let model = &mut app.core.model;
        model.nodes[1].restraint = Dof6Mask::FIXED;
        model.nodes[1].restraint.set_free(Dof::Uz);
        let mut brace_base = model.nodes[0].clone();
        brace_base.id = NodeId(2);
        brace_base.coord[2] = -3000.0;
        model.nodes.push(brace_base);

        let mut steel = model.materials[1].clone();
        steel.id = MaterialId(2);
        steel.category = MaterialCategory::Steel;
        steel.name = "SN400B".into();
        steel.young = 200000.0;
        steel.fy = Some(235.0);
        model.materials.push(steel);
        let mut section = Section::zero(SectionId(1), "引張専用ブレース".into());
        section.frame_use = Some(FrameSectionUse::Brace);
        section.material = Some(MaterialId(2));
        section.area = 60000.0;
        section.thickness = Some(10.0);
        model.sections.push(section);
        let mut brace = model.elements[0].clone();
        brace.id = ElemId(1);
        brace.kind = ElementKind::Brace { tension_only: true };
        brace.nodes = [NodeId(2), NodeId(1)].into_iter().collect();
        brace.section = Some(SectionId(1));
        model.elements.push(brace);

        model.load_cases.truncate(2);
        model.load_cases[1].kind = LoadCaseKind::Live;
        for (case, axial_load_n) in model.load_cases.iter_mut().zip([-4000.0, 8000.0]) {
            case.nodal[0].values = [0.0, 0.0, axial_load_n, 0.0, 0.0, 0.0];
        }
        model.combinations[0].terms.truncate(2);
        model.stress_cfg.tension_only_iteration = true;

        let solve_gravity_cases = |app: &mut App| {
            for case in [LoadCaseId(0), LoadCaseId(1)] {
                app.run_linear_static(case);
                assert!(
                    app.core.scoped.last_error.is_none(),
                    "{:?}",
                    app.core.scoped.last_error
                );
            }
        };
        solve_gravity_cases(&mut app);
        let axial_forces = |case| {
            let result = &app
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .statics
                .iter()
                .find(|(key, _)| *key == StaticCaseKey::User(LoadCaseId(case)))
                .unwrap()
                .1;
            [ElemId(0), ElemId(1)].map(|elem| {
                result
                    .member_forces
                    .iter()
                    .find(|(id, _)| *id == elem)
                    .unwrap()
                    .1
                    .at[0]
                    .1[0]
            })
        };
        let [column_g_n, brace_g_n] = axial_forces(0);
        let [column_p_n, brace_p_n] = axial_forces(1);
        assert!(brace_g_n.abs() < 0.01);
        close(column_g_n + brace_g_n, -4000.0);
        assert!(brace_p_n > 0.0 && column_p_n > 0.0);
        close(column_p_n + brace_p_n, 8000.0);
        if !cft {
            // RC柱とブレースの EA/L が等しいので、引張荷重は等分される。
            close(brace_p_n, 4000.0);
            assert!((column_g_n + column_p_n).abs() < 0.01);
        }

        let error = app_error(&mut app, cft);
        for expected in [
            "gravity_superposition_unsupported:",
            "引張専用ブレースの反復解析",
            "有効状態が同一と確認できない",
            "mode=static_hinge",
            "purpose=frame_long",
            "selected_terms=[case=0 coefficient=1, case=1 coefficient=1]",
        ] {
            assert!(error.contains(expected), "missing {expected}: {error}");
        }

        app.core.model.stress_cfg.tension_only_iteration = false;
        solve_gravity_cases(&mut app);
        let demand = app.ultimate_demand_by_elem().unwrap();
        assert_eq!(demand[0].1.mode, UltimateDemandMode::StaticHinge);
        if cft {
            close(
                app.compute_cft_ultimate_checks().unwrap()[0].n_design,
                -column_p_n / 2.0,
            );
        } else {
            close(demand[0].1.n_axial, -2000.0);
            assert_eq!(app.compute_ultimate_checks().unwrap().len(), 1);
        }
    }
}

#[cfg(feature = "gui")]
#[test]
fn static_rc_biaxial_table_distinguishes_ok_ng_and_unchecked() {
    for (load, status, margin, summary) in [
        (
            0.0,
            "OK",
            "二軸 ",
            "対象部材の限定検定 OK（全体判定は未完了）",
        ),
        (12000.0, "未検定", "強軸 ", "未検定 1 本"),
        (500000.0, "NG", "強軸 ", "NG 1 本"),
    ] {
        let mut app = column(false);
        for case in &mut app.core.model.load_cases {
            case.nodal[0].values[0] = 0.0;
        }
        app.core.model.load_cases[0].nodal[0].values[0] = load;
        app.core.ultimate_biaxial_shear = true;
        app.core.ultimate_include_bond = false;
        solve_statics(&mut app);

        let ctx = egui::Context::default();
        let frame = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            ..Default::default()
        };
        let mut draw = |ui: &mut egui::Ui| crate::ultimate_view::ultimate_table(ui, &mut app);
        let _ = ctx.run_ui(frame(), &mut draw);
        let output = ctx.run_ui(frame(), draw);
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&status), "load={load}: {labels:?}");
        assert!(labels.contains(&summary), "load={load}: {labels:?}");
        assert!(labels.iter().any(|s| s.starts_with(margin)), "{labels:?}");
        assert_eq!(
            labels
                .iter()
                .any(|s| s.starts_with("対象部材の限定検定 OK")),
            status == "OK",
            "{labels:?}"
        );
        assert_eq!(labels.contains(&"NG 1 本"), status == "NG");
        assert_eq!(labels.contains(&"未検定 1 本"), status == "未検定");
        assert_eq!(
            labels.iter().any(|s| s.starts_with("二軸 ")),
            status == "OK"
        );
    }
}

#[test]
fn actual_rc_cft_confirmed_run_reaches_app_checks_without_ql_addition_or_fallback() {
    for cft in [false, true] {
        let mut app = column(cft);
        if !cft {
            app.core.model.load_cases[0].nodal[0].values[1] = 6000.0;
        }
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
        let po = bundle.pushover_x.as_mut().unwrap();
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
        assert_eq!(d.q_long_weak, None);
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
        if !cft {
            close(d.shear_weak.unwrap().abs(), 6000.0);
            app.core.ultimate_biaxial_shear = true;
            let check = &app.compute_ultimate_checks().unwrap()[0];
            assert_eq!(check.unchecked_reason, None);
            assert!(check.biaxial_shear_margin.unwrap() < check.shear_margin);
            close(check.qmu, d.shear.unwrap().abs());
        }
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
        bundle.pushover_x = Some(old_run);

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

fn beam_model(top_bars: &[u32], bottom_bars: &[u32]) -> sepika_core::model::Model {
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar};
    let mut model = column(false).core.model;
    let mut base = model.nodes[0].clone();
    base.id = NodeId(2);
    base.coord = [0.0, 6000.0, 3000.0];
    let mut tip = model.nodes[1].clone();
    tip.id = NodeId(3);
    tip.coord = [6000.0, 6000.0, 3000.0];
    model.nodes.extend([base, tip]);
    let mut beam = model.elements[0].clone();
    beam.id = ElemId(1);
    beam.nodes = [NodeId(2), NodeId(3)].into_iter().collect();
    beam.local_axis.ref_vector = [0.0, 0.0, 1.0];
    beam.section = Some(SectionId(1));
    model.elements.push(beam);
    let shape = SectionShape::RcBeamRect {
        b: 400.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: 22.0,
            top: top_bars.to_vec(),
            bottom: bottom_bars.to_vec(),
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        },
    };
    let mut section = shape.to_section(SectionId(1), "静的終局検定梁".into());
    section.frame_use = Some(FrameSectionUse::Girder);
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = Some(MaterialId(1));
    model.sections.push(section);
    model
}

fn solve_beam_model(model: sepika_core::model::Model) -> App {
    let mut app = App::default();
    assert!(app.load_model(model));
    app.core.analysis_cfg.threads = 1;
    app.core.ultimate_upper_factor = 1.0;
    app.core.ultimate_include_bond = true;
    app.core.ultimate_shear_ductility = false;
    solve_statics(&mut app);
    app
}

fn static_case_forces(
    app: &App,
    load_case: u32,
) -> &[(ElemId, sepika_element::frame::beam::MemberForces)] {
    &app.core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .statics
        .iter()
        .find(|(key, _)| *key == StaticCaseKey::User(LoadCaseId(load_case)))
        .unwrap()
        .1
        .member_forces
}

fn beam_forces(app: &App, load_case: u32) -> &sepika_element::frame::beam::MemberForces {
    &static_case_forces(app, load_case)
        .iter()
        .find(|(elem, _)| *elem == ElemId(1))
        .unwrap()
        .1
}

#[test]
fn actual_rc_beam_static_check_uses_bending_sign_after_gravity_superposition() {
    // b/D=400/600、dt=61 mm、SD295・Fc24。引張側の付着信頼強度は上端4-D22で
    // 8.253952 N/mm²（上端筋係数0.69）、下端2-D22・3-D22で12.038422 N/mm²。
    for (direction, top, bottom, expected_mu, expected_qbu, can_pass) in [
        (-1.0, 4, 2, 217_594_806.213, 1_091_221.650, true),
        (1.0, 4, 2, 108_797_403.107, 818_332.214, false),
        (1.0, 2, 3, 163_196_104.660, 1_185_842.203, true),
    ] {
        let mut model = beam_model(&[top], &[bottom]);
        for (index, force_n) in [(0, -20_000.0), (2, 30_000.0)] {
            let shear_n = direction * force_n;
            let moment_n_mm = 750.0 * shear_n;
            model.load_cases[index].nodal.push(NodalLoad::manual(
                NodeId(3),
                [0.0, 0.0, shear_n, 0.0, -moment_n_mm, 0.0],
            ));
        }
        let mut app = solve_beam_model(model);
        close(beam_forces(&app, 0).at[0].1[5], -direction * 135e6);
        close(
            beam_forces(&app, 0).at.last().unwrap().1[5],
            -direction * 15e6,
        );
        close(beam_forces(&app, 2).at[0].1[5], direction * 202.5e6);
        close(
            beam_forces(&app, 2).at.last().unwrap().1[5],
            direction * 22.5e6,
        );
        let demand = app
            .ultimate_demand_by_elem()
            .unwrap()
            .into_iter()
            .find(|(elem, _)| *elem == ElemId(1))
            .unwrap()
            .1;
        close(demand.mz, direction * 67.5e6);
        close(demand.q_long.unwrap(), 10_000.0);
        let checks = app.compute_ultimate_checks().unwrap();
        let beam = checks.iter().find(|check| check.elem == ElemId(1)).unwrap();
        close(beam.mu, expected_mu);
        close(beam.qbu, expected_qbu);
        assert_eq!(beam.ok, can_pass);
        if can_pass {
            assert_eq!(beam.unchecked_reason, None);
            close(beam.qmu, 2.0 * expected_mu / 6000.0);
            close(beam.bond_margin, (expected_qbu - 10_000.0) / beam.qmu);
        } else {
            let reason = beam.unchecked_reason.as_deref().unwrap();
            assert!(
                reason.contains("ultimate_beam_hinge_bending_unverified:"),
                "{reason}"
            );
            assert!(beam.qmu.is_nan() && beam.bond_margin.is_nan());
        }
    }
}

#[cfg(feature = "gui")]
fn ultimate_table_labels(app: &mut App) -> Vec<String> {
    let output = egui::Context::default().run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1800.0, 1200.0),
            )),
            ..Default::default()
        },
        |ui| crate::ultimate_view::ultimate_table(ui, app),
    );
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn actual_rc_beam_mixed_static_bending_leaves_only_the_beam_unchecked() {
    use sepika_core::dof::Dof6Mask;
    use sepika_core::model::{MemberLoad, MemberLoadKind};

    let mut model = beam_model(&[4], &[2]);
    model.nodes[3].restraint = Dof6Mask::FIXED;
    model.load_cases[0].member.push(MemberLoad::manual(
        ElemId(1),
        [0.0, 0.0, -1.0],
        MemberLoadKind::Distributed {
            a: 0.0,
            b: 6000.0,
            w1: 10.0,
            w2: 10.0,
        },
    ));
    let mut app = solve_beam_model(model);
    let forces = &beam_forces(&app, 0).at;
    close(forces.first().unwrap().1[5], -30_000_000.0);
    close(forces.last().unwrap().1[5], -30_000_000.0);
    assert!(forces.iter().any(|(_, f)| f[5] > 0.0));

    let checks = app.compute_ultimate_checks().unwrap();
    let beam = checks.iter().find(|check| check.elem == ElemId(1)).unwrap();
    assert!(!beam.ok);
    assert!(beam.mu.is_nan() && beam.qbu.is_nan());
    let reason = beam.unchecked_reason.as_deref().unwrap();
    for evidence in [
        "ultimate_beam_mz_mixed_signs:",
        "member=1",
        "case=0 coefficient=1",
    ] {
        assert!(reason.contains(evidence), "{reason}");
    }
    let column = checks.iter().find(|check| check.elem == ElemId(0)).unwrap();
    assert_eq!(column.unchecked_reason, None);
    assert!(column.mu > 0.0 && column.qbu > 0.0);
    #[cfg(feature = "gui")]
    {
        let labels = ultimate_table_labels(&mut app);
        assert!(
            labels.iter().any(|label| label == "未検定 1 本"),
            "{labels:?}"
        );
        assert!(labels.iter().any(|label| label == "未検定"), "{labels:?}");
        assert!(labels.iter().any(|label| label.contains("両端ヒンジ略算")));
        assert_eq!(
            labels.iter().filter(|&label| label == "-").count(),
            6,
            "{labels:?}"
        );
        assert!(
            !labels.iter().any(|label| label.contains("NaN")),
            "{labels:?}"
        );
    }
}

#[test]
fn actual_rc_beam_without_static_bending_has_no_tension_side() {
    for bottom_bars in [2, 4] {
        let mut app = solve_beam_model(beam_model(&[4], &[bottom_bars]));
        let checks = app.compute_ultimate_checks().unwrap();
        let beam = checks.iter().find(|check| check.elem == ElemId(1)).unwrap();
        assert!(!beam.ok);
        assert!(beam.mu.is_nan() && beam.qbu.is_nan());
        let reason = beam.unchecked_reason.as_deref().unwrap();
        assert!(reason.contains("ultimate_beam_mz_zero:"), "{reason}");
    }
}

#[test]
fn public_unsigned_static_forces_leave_only_the_rc_beam_unchecked() {
    use sepika_design_jp::ultimate::collect_rc_ultimate_checks;

    let mut model = beam_model(&[4], &[2]);
    model.load_cases[0].nodal.push(NodalLoad::manual(
        NodeId(3),
        [0.0, 0.0, 0.0, 0.0, 20e6, 0.0],
    ));
    let app = solve_beam_model(model);
    close(beam_forces(&app, 0).at[0].1[5], -20e6);
    let forces = static_case_forces(&app, 0);
    let q_long = sepika_job::q_long_map_from_member_forces(forces);
    let demand = sepika_job::member_demand_from_static_forces(forces, Some(&q_long));
    let checks = collect_rc_ultimate_checks(&app.core.model, &demand, &Default::default()).unwrap();
    let beam = checks.iter().find(|check| check.elem == ElemId(1)).unwrap();
    assert!(!beam.ok);
    assert!(beam.mu.is_nan());
    let reason = beam.unchecked_reason.as_deref().unwrap();
    assert!(reason.contains("ultimate_beam_mz_unverified:"), "{reason}");
    let column = checks.iter().find(|check| check.elem == ElemId(0)).unwrap();
    assert_eq!(column.unchecked_reason, None);
    assert!(column.mu > 0.0);
}

#[test]
fn actual_rc_beam_without_reinforcement_on_the_tension_side_leaves_only_the_beam_unchecked() {
    for (top, bottom, moment_n_mm, side) in [
        (&[][..], &[2][..], -20e6, "上端"),
        (&[4][..], &[][..], 20e6, "下端"),
    ] {
        let mut model = beam_model(top, bottom);
        model.load_cases[0].nodal.push(NodalLoad::manual(
            NodeId(3),
            [0.0, 0.0, 0.0, 0.0, -moment_n_mm, 0.0],
        ));
        let mut app = solve_beam_model(model);
        close(beam_forces(&app, 0).at[0].1[5], moment_n_mm);
        let checks = app.compute_ultimate_checks().unwrap();
        let beam = checks.iter().find(|check| check.elem == ElemId(1)).unwrap();
        assert!(!beam.ok);
        assert!(beam.mu.is_nan() && beam.shear_margin.is_nan() && beam.bond_margin.is_nan());
        let reason = beam.unchecked_reason.as_deref().unwrap();
        assert!(
            reason.contains("ultimate_beam_tension_rebar_missing:"),
            "{reason}"
        );
        assert!(reason.contains(side), "{reason}");
        let column = checks.iter().find(|check| check.elem == ElemId(0)).unwrap();
        assert_eq!(column.unchecked_reason, None);
        assert!(column.mu > 0.0);
    }
}

#[test]
fn actual_rc_beam_with_stronger_opposite_bending_keeps_capacities_and_static_failure() {
    // 内法3m、上2-D22・下4-D22、2-D10@250、Rp=.05。上端選択時の旧略算需要は
    // 72.532 kN。両端が異なる向きに降伏する場合を仮定すると108.797 kNで、
    // QL=10 kNでは控除後のQsu=107.756 kNとの大小が逆転する。
    for (ql_n, static_failure) in [(10_000.0, false), (50_000.0, true)] {
        let mut model = beam_model(&[2], &[4]);
        model.nodes[3].coord[0] = 3000.0;
        let Some(SectionShape::RcBeamRect { rebar, .. }) = model.sections[1].shape.as_mut() else {
            unreachable!();
        };
        rebar.stirrup.pitch = 250.0;
        model.load_cases[0].nodal.push(NodalLoad::manual(
            NodeId(3),
            [0.0, 0.0, -ql_n, 0.0, 5e6, 0.0],
        ));
        let mut app = solve_beam_model(model);
        app.core.ultimate_rp = 0.05;
        close(beam_forces(&app, 0).at[0].1[5], -3_000.0 * ql_n - 5e6);
        close(beam_forces(&app, 0).at.last().unwrap().1[5], -5e6);
        let checks = app.compute_ultimate_checks().unwrap();
        let beam = checks.iter().find(|check| check.elem == ElemId(1)).unwrap();
        let column = checks.iter().find(|check| check.elem == ElemId(0)).unwrap();
        close(beam.mu, 108_797_403.107);
        close(beam.qsu, 117_755.918);
        close(beam.qbu, 278_854.410);
        assert!(!beam.ok);
        assert_eq!(column.unchecked_reason, None);
        assert!(column.mu > 0.0);
        if static_failure {
            assert_eq!(beam.unchecked_reason, None);
            close(beam.qmu, 72_531.602);
            assert!(beam.shear_margin < 1.0);
            assert!(beam.bond_margin > 1.0);
        } else {
            let reason = beam.unchecked_reason.as_deref().unwrap();
            assert!(
                reason.contains("ultimate_beam_hinge_bending_unverified:"),
                "{reason}"
            );
            assert!(beam.qmu.is_nan() && beam.shear_margin.is_nan() && beam.bond_margin.is_nan());
            #[cfg(feature = "gui")]
            {
                let labels = ultimate_table_labels(&mut app);
                for text in ["108.8", "117.8", "278.9", "未検定"] {
                    assert!(labels.iter().any(|label| label == text), "{labels:?}");
                }
                assert_eq!(
                    labels.iter().filter(|&label| label == "-").count(),
                    3,
                    "{labels:?}"
                );
            }
        }
    }
}

#[test]
fn actual_rc_beam_leaves_check_pending_when_opposite_shear_or_bond_capacity_is_insufficient() {
    // 内法3m、下端引張。上1-D22/下2-D22では QL + Qmu = 422.5 kN に対し
    // 上端の Qbu = 402.1 kN。上2段各1-D22/下3-D22では QL + Qmu = 563.8 kN に対し
    // 上端の Qsu = 552.7 kN。どちらも下端の Qsu・Qbu と比較すれば OK になる。
    for (top, bottom, ql_n, expected_mu, expected_qbu, cause) in [
        (
            &[1][..],
            &[2][..],
            350_000.0,
            108_797_403.107,
            851_354.788,
            "ultimate_beam_hinge_bond_unverified:",
        ),
        (
            &[1, 1][..],
            &[3][..],
            455_000.0,
            163_196_104.660,
            1_194_331.084,
            "ultimate_beam_hinge_shear_unverified:",
        ),
    ] {
        let mut model = beam_model(top, bottom);
        model.nodes[3].coord[0] = 3000.0;
        model.load_cases[0].nodal.push(NodalLoad::manual(
            NodeId(3),
            [0.0, 0.0, ql_n, 0.0, -5e6, 0.0],
        ));
        let mut app = solve_beam_model(model);
        app.core.ultimate_rp = 0.0;
        close(beam_forces(&app, 0).at[0].1[5], 3_000.0 * ql_n + 5e6);
        close(beam_forces(&app, 0).at.last().unwrap().1[5], 5e6);
        let checks = app.compute_ultimate_checks().unwrap();
        let beam = checks.iter().find(|check| check.elem == ElemId(1)).unwrap();
        close(beam.mu, expected_mu);
        close(beam.qsu, 574_959.528);
        close(beam.qbu, expected_qbu);
        assert!(!beam.ok);
        let reason = beam.unchecked_reason.as_deref().unwrap();
        assert!(reason.contains(cause), "{reason}");
        assert!(beam.qmu.is_nan() && beam.shear_margin.is_nan() && beam.bond_margin.is_nan());
    }
}
