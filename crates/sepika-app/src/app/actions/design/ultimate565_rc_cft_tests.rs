use super::*;
use sepika_core::ids::MaterialId;
use sepika_core::model::{
    FrameSectionUse, LoadCase, LoadCaseKind, LoadCombination, MaterialCategory, NodalLoad,
};
use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};
use sepika_design_jp::ultimate::UltimateDemandMode;
use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;

#[path = "ultimate_physical_beam_tests.rs"]
mod physical_beam;

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
                main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
                x: vec![3],
                y: vec![2],
                cover: 40.0,
                hoop: RectColumnHoop {
                    dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
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
        if let Some(q) = total_q {
            close(c.qmu, q.abs());
            // Fc24・400×600、主筋3-D22相当、補強筋2-D10相当@150、N=150 kN、M/(Qd)の上限3。
            let expected_capacity_n = 301_867.305_080_403_2;
            close(c.qsu, expected_capacity_n);
            close(c.shear_margin, expected_capacity_n / q.abs());
        } else {
            assert!(c.qmu > 0.0);
            assert!(c.qsu.is_nan() && c.shear_margin.is_nan());
            assert!(c.detail.contains("強軸せん断余裕度=未算定") && !c.detail.contains("max("));
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
            close(r.rc_ultimate_shear.unwrap().axial_range_n[0], 150000.0);
            assert_eq!(d.rc_ultimate_shear, r.rc_ultimate_shear);
            close(d.shear.unwrap(), 39_694.589_12);
            #[cfg(feature = "gui")]
            {
                let labels = ultimate_table_labels(&mut app);
                for text in ["301.9", "7.60"] {
                    assert!(labels.iter().any(|label| label == text), "{labels:?}");
                }
            }
            close(d.shear_weak.unwrap().abs(), 6000.0);
            app.core.ultimate_biaxial_shear = true;
            let check = &app.compute_ultimate_checks().unwrap()[0];
            assert!(check
                .unchecked_reason
                .as_deref()
                .unwrap()
                .contains("終局付着を未算定"));
            assert!(check.weak_shear_margin.unwrap() > 0.0);
            assert!(check
                .detail
                .contains("二軸同時作用に対する終局せん断は未算定"));
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
            main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
            top: top_bars.to_vec(),
            bottom: bottom_bars.to_vec(),
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
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
fn actual_rc_beam_reports_reference_strengths_from_signed_gravity_without_passing_a_free_end() {
    // b/D=400/600、dt=61 mm、SD295・Fc24。
    for (direction, top, bottom, expected_mu) in [
        (-1.0, 4, 2, 217_594_806.213),
        (1.0, 4, 2, 108_797_403.107),
        (1.0, 2, 3, 163_196_104.660),
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
        assert!(beam.bond_stress.is_none());
        assert!(!beam.ok);
        let reason = beam.unchecked_reason.as_deref().unwrap();
        assert!(
            reason.contains("ultimate_beam_hinge_free_end: j端"),
            "{reason}"
        );
        assert!(
            reason.contains("ultimate_beam_hinge_shear_upper_bound_unverified:"),
            "{reason}"
        );
        assert!(beam.detail.contains("参考曲げ略算 Mu"));
        assert!(beam.qmu.is_nan() && beam.shear_margin.is_nan() && beam.bond_margin.is_nan());
    }
}

#[cfg(feature = "gui")]
fn ultimate_table_frame(
    ctx: &egui::Context,
    app: &mut App,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1800.0, 1200.0),
            )),
            events,
            ..Default::default()
        },
        |ui| crate::ultimate_view::ultimate_table(ui, app),
    )
}

#[cfg(feature = "gui")]
fn ultimate_table_labels(app: &mut App) -> Vec<String> {
    ultimate_table_frame(&egui::Context::default(), app, vec![])
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

#[cfg(feature = "gui")]
fn ultimate_table_hover(
    app: &mut App,
    label: &str,
    occurrence: usize,
    detail_fragment: &str,
) -> String {
    let ctx = egui::Context::default();
    ctx.global_style_mut(|style| {
        style.interaction.tooltip_delay = 0.0;
        style.interaction.show_tooltips_only_when_still = false;
    });
    ultimate_table_frame(&ctx, app, vec![]);
    let position = ultimate_table_frame(&ctx, app, vec![])
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                Some(text.galley.rect.translate(text.pos.to_vec2()).center())
            }
            _ => None,
        })
        .nth(occurrence)
        .unwrap_or_else(|| panic!("一覧に{}個目の「{label}」がありません", occurrence + 1));
    ultimate_table_frame(&ctx, app, vec![egui::Event::PointerMoved(position)]);
    for _ in 0..3 {
        let output = ultimate_table_frame(&ctx, app, vec![]);
        if let Some(detail) = output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text().contains(detail_fragment) => {
                Some(text.galley.text().to_owned())
            }
            _ => None,
        }) {
            return detail;
        }
    }
    panic!("「{label}」のホバー詳細に「{detail_fragment}」がありません");
}

#[cfg(feature = "gui")]
#[test]
fn rc_ultimate_table_and_bond_hover_distinguish_disabled_and_unavailable_bond() {
    let mut app = column(false);
    let mut round_bars = app.core.model.materials[1].clone();
    round_bars.id = MaterialId(2);
    round_bars.name = "SR235".into();
    round_bars.fy = Some(235.0);
    app.core.model.materials.push(round_bars);
    app.core.model.sections[0].rebar_material = Some(MaterialId(2));
    solve_statics(&mut app);

    for (include_bond, status, verdict) in [(false, "無効", "未検定"), (true, "未算定", "未検定")]
    {
        app.core.ultimate_include_bond = include_bond;
        let labels = ultimate_table_labels(&mut app);
        assert_eq!(
            labels.iter().filter(|&label| label == status).count(),
            4,
            "{labels:?}"
        );
        assert!(labels.iter().any(|label| label == "必要延長"), "{labels:?}");
        assert!(labels.iter().any(|label| label == verdict), "{labels:?}");
        let check = &app.compute_ultimate_checks().unwrap()[0];
        if include_bond {
            assert!(check.bond_stress.is_none() && check.bond_margin.is_nan());
        } else {
            assert!(check.bond_stress.is_none());
            assert_eq!(check.bond_margin, f64::INFINITY);
        }

        let detail = ultimate_table_hover(&mut app, "0", 0, "τD=");
        for expected in [
            format!("τD={status}, Kfb={status}"),
            format!("付着余裕度={status}"),
        ] {
            assert!(detail.contains(&expected), "{expected}: {detail}");
        }
        assert_eq!(
            detail.contains("ultimate_bond_round_bar_unsupported:") && detail.contains("SR235"),
            include_bond,
            "{detail}"
        );
        if include_bond {
            let reason =
                ultimate_table_hover(&mut app, status, 3, "ultimate_bond_round_bar_unsupported:");
            assert!(reason.contains("SR235"), "{reason}");
        }
    }
}

#[cfg(feature = "gui")]
#[test]
fn rc_column_with_no_clear_span_keeps_its_unchecked_reason_in_the_ultimate_table() {
    use sepika_core::model::{GirderTorsionMode, PanelZoneMode, ZoneSource};

    let mut model = beam_model(&[3], &[3]);
    model.girder_torsion = GirderTorsionMode::Keep;
    model.panel_zone = PanelZoneMode::None;
    model.nodes[1].coord = [0.0, 0.0, 200.0];
    model.nodes.swap_remove(2);
    model.nodes[2].id = NodeId(2);
    model.nodes[2].coord = [6000.0, 0.0, 200.0];
    model.elements[1].nodes = [NodeId(1), NodeId(2)].into_iter().collect();
    for element in &mut model.elements {
        element.rigid_zone.source_i = ZoneSource::Manual;
        element.rigid_zone.source_j = ZoneSource::Manual;
    }
    model.load_cases[0].nodal.push(NodalLoad::manual(
        NodeId(2),
        [0.0, 0.0, 0.0, 0.0, 10e6, 0.0],
    ));
    let mut app = solve_beam_model(model);
    let checks = app.compute_ultimate_checks().unwrap();
    let column = checks.iter().find(|check| check.elem == ElemId(0)).unwrap();
    let beam = checks.iter().find(|check| check.elem == ElemId(1)).unwrap();
    assert!(beam
        .unchecked_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("ultimate_beam_hinge_free_end:")));
    assert!(beam.qsu.is_nan());
    assert!(!column.ok);
    assert!(column.qsu.is_nan() && column.bond_margin.is_nan());
    let reason = column.unchecked_reason.as_deref().unwrap();
    let cause = [
        "ultimate_clear_span_invalid:",
        "材軸長 200 mm",
        "j端フェース距離 300 mm",
    ];
    for expected in cause.into_iter().chain(["member=0"]) {
        assert!(reason.contains(expected), "{reason}");
    }
    #[cfg(feature = "gui")]
    {
        ultimate_table_hover(&mut app, "未検定", 0, reason);
        for (include_bond, status, count) in [(false, "無効", 8), (true, "未算定", 8)] {
            app.core.ultimate_include_bond = include_bond;
            let labels = ultimate_table_labels(&mut app);
            assert_eq!(
                labels.iter().filter(|&label| label == status).count(),
                count,
                "{labels:?}"
            );
            let detail = if include_bond {
                ultimate_table_hover(&mut app, status, 3, "ultimate_clear_span_invalid:")
            } else {
                ultimate_table_hover(&mut app, "0", 0, "τD=")
            };
            for expected in cause {
                assert!(detail.contains(expected), "{expected}: {detail}");
            }
            assert!(
                detail.contains(&format!("τD={status}, Kfb={status}, 付着余裕度={status}")),
                "{detail}"
            );
        }
    }
}

#[test]
fn actual_rc_beam_mixed_static_bending_withholds_reference_mu() {
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
    assert!(beam.mu.is_nan() && beam.bond_stress.is_none());
    let reason = beam.unchecked_reason.as_deref().unwrap();
    for evidence in [
        "ultimate_beam_mz_mixed_signs:",
        "member=1",
        "case=0 coefficient=1",
    ] {
        assert!(reason.contains(evidence), "{reason}");
    }
    let column = checks.iter().find(|check| check.elem == ElemId(0)).unwrap();
    assert!(column.unchecked_reason.is_some());
    assert!(column.mu > 0.0 && column.bond_stress.is_none());
    #[cfg(feature = "gui")]
    {
        let labels = ultimate_table_labels(&mut app);
        assert!(
            labels.iter().any(|label| label == "未検定 2 本"),
            "{labels:?}"
        );
        assert!(labels.iter().any(|label| label == "未検定"), "{labels:?}");
        assert!(labels.iter().any(|label| label.contains("両端ヒンジ略算")));
        assert_eq!(
            labels.iter().filter(|&label| label == "-").count(),
            6,
            "{labels:?}"
        );
        assert_eq!(
            labels.iter().filter(|&label| label == "未算定").count(),
            8,
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
        assert!(beam.mu.is_nan() && beam.bond_stress.is_none());
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
    assert!(column.unchecked_reason.is_some());
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
        assert!(column.unchecked_reason.is_some());
        assert!(column.mu > 0.0);
    }
}

#[test]
fn actual_rc_free_end_never_shows_an_ok_or_ng_from_an_inapplicable_hinge_demand() {
    // 内法3m、上2-D22・下4-D22、2-D10@250、Rp=.05。かつての比較需要は72.532 kNで、
    // QL=10 kNではQsu=117.756 kN以下、QL=50 kNでは比較を満たさない。
    for ql_n in [10_000.0, 50_000.0] {
        let mut model = beam_model(&[2], &[4]);
        model.nodes[3].coord[0] = 3000.0;
        let Some(SectionShape::RcBeamRect { rebar, .. }) = model.sections[1].shape.as_mut() else {
            unreachable!();
        };
        rebar.stirrup.pitch = 250.0;
        model.load_cases[0].nodal.push(NodalLoad::manual(
            NodeId(3),
            [0.0, 0.0, -ql_n, 0.0, 0.0, 0.0],
        ));
        model.load_cases[0].nodal.push(NodalLoad::manual(
            NodeId(1),
            [3_000_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ));
        let mut app = solve_beam_model(model);
        close(beam_forces(&app, 0).at[0].1[5], -3_000.0 * ql_n);
        close(beam_forces(&app, 0).at.last().unwrap().1[5], 0.0);
        let checks = app.compute_ultimate_checks().unwrap();
        let beam = checks.iter().find(|check| check.elem == ElemId(1)).unwrap();
        let column = checks.iter().find(|check| check.elem == ElemId(0)).unwrap();
        close(beam.mu, 108_797_403.107);
        assert!(beam.qsu.is_nan());
        assert!(beam.bond_stress.is_none());
        assert!(!beam.ok);
        assert!(column.unchecked_reason.is_some());
        assert!(!column.ok && column.shear_margin.is_nan());
        let reason = beam.unchecked_reason.as_deref().unwrap();
        let cause = "ultimate_beam_hinge_free_end: j端";
        assert!(reason.contains(cause), "{reason}");
        assert!(beam.qmu.is_nan() && beam.shear_margin.is_nan() && beam.bond_margin.is_nan());
        #[cfg(feature = "gui")]
        {
            let labels = ultimate_table_labels(&mut app);
            for text in ["108.8（参考）", "未算定", "未検定", "未検定 2 本"] {
                assert!(labels.iter().any(|label| label == text), "{labels:?}");
            }
            assert_eq!(
                labels.iter().filter(|&label| label == "-").count(),
                5,
                "{labels:?}"
            );
            let detail = ultimate_table_hover(&mut app, "1", 0, "τD=");
            for expected in ["τD=未算定", "付着余裕度=未算定", cause] {
                assert!(detail.contains(expected), "{expected}: {detail}");
            }
        }
    }
}

#[test]
fn actual_rc_beam_with_verified_zero_axial_force_displays_arakawa_mean_capacity() {
    use sepika_core::model::{ForceRegime, RcBeamReference};

    let mut model = beam_model(&[4], &[4]);
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut model.sections[1].shape else {
        unreachable!()
    };
    rebar.main_dia = sepika_core::section_shape::RebarSize::Standard(
        sepika_core::section_shape::StandardRebar::D22,
    );
    model.nodes[3]
        .restraint
        .set_fixed(sepika_core::dof::Dof::Ux);
    model.nodes[3]
        .restraint
        .set_fixed(sepika_core::dof::Dof::Uy);
    model.elements[1].force_regime = ForceRegime::UniaxialBendingShear;
    model.set_member_rc_beam_reference(
        ElemId(1),
        Some(RcBeamReference::AntisymmetricHalfMemberWithoutAxialForce),
    );
    model.load_cases[0].nodal.push(NodalLoad::manual(
        NodeId(3),
        [0.0, 0.0, -10_000.0, 0.0, 0.0, 0.0],
    ));
    let mut app = solve_beam_model(model);
    app.generate_stories_action();
    app.run_seismic(SeismicDir::Y);
    app.core.analysis_cfg.push_dir = SeismicDir::Y;
    app.core.analysis_cfg.push_steps = 3;
    app.run_pushover();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app.core.ultimate_use_pushover = true;
    let po = app
        .core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .pushover_y
        .as_mut()
        .unwrap();
    let response = &po.confirmed_history.as_ref().unwrap()[1];
    let member = response
        .members
        .iter()
        .find(|member| member.elem == ElemId(1))
        .unwrap();
    let actions = member.rc_beam_ultimate_shear.unwrap().unwrap();
    assert_eq!(actions.axial_range_n, [0.0; 2]);
    po.capacity_evaluation = Some(
        po.evaluation_point(
            EvaluationPurpose::HoldingCapacity,
            SeismicDir::Y,
            response.step,
            "RC梁の軸力なしを確認したstep".into(),
        )
        .unwrap(),
    );

    let demands = app.ultimate_demand_by_elem().unwrap();
    let demand = &demands.iter().find(|(id, _)| *id == ElemId(1)).unwrap().1;
    assert_eq!(demand.rc_beam_ultimate_shear, Some(Ok(actions)));
    let checks = app.compute_ultimate_checks().unwrap();
    let beam = checks.iter().find(|check| check.elem == ElemId(1)).unwrap();
    // Fc24・400×600、主筋4-D22相当、補強筋2-D10相当@100、N=0、M/(Qd)の上限3。
    close(beam.qmu, 10_000.0);
    close(beam.qsu, 332_617.813_860_871_25);
    close(beam.shear_margin, 33.261_781_386_087_13);
    #[cfg(feature = "gui")]
    {
        let labels = ultimate_table_labels(&mut app);
        for text in ["332.6", "33.26"] {
            assert!(labels.iter().any(|label| label == text), "{labels:?}");
        }
    }

    for has_shear_evidence in [true, false] {
        if !has_shear_evidence {
            let po = app
                .core
                .scoped
                .results
                .as_mut()
                .unwrap()
                .pushover_y
                .as_mut()
                .unwrap();
            for response in po.confirmed_history.as_mut().unwrap() {
                for member in &mut response.members {
                    member.rc_beam_ultimate_shear = None;
                }
            }
        }
        let path = std::env::temp_dir().join(format!(
            "sepika1873-face-{has_shear_evidence}-{}.ovika",
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
        loaded.core.ultimate_use_pushover = true;
        let checks = loaded.compute_ultimate_checks().unwrap();
        let beam = checks.iter().find(|check| check.elem == ElemId(1)).unwrap();
        close(beam.qmu, 10_000.0);
        if has_shear_evidence {
            close(beam.qsu, 332_617.813_860_871_25);
            close(beam.shear_margin, 33.261_781_386_087_13);
        } else {
            assert!(beam.qsu.is_nan() && beam.shear_margin.is_nan());
            assert!(
                beam.mu.is_finite() && beam.detail.contains("増分解析を再実行してください"),
                "{beam:?}"
            );
        }
    }
}

#[cfg(feature = "gui")]
#[test]
fn rc_ultimate_ovika_rejects_retired_settings_and_old_response_contract() {
    use sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION;
    use sepika_io::ovika::{load_ovika, save_ovika, OvikaExtras};

    let path = std::env::temp_dir().join(format!("sepika1871-{}.ovika", std::process::id()));
    let mut app = column(false);
    app.core.model.load_cases[0].nodal[0].values[1] = 6000.0;
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
    let static_labels = ultimate_table_labels(&mut app).join("\n");
    assert!(
        static_labels.contains("静的・両端ヒンジ略算では終局せん断は未算定"),
        "{static_labels}"
    );

    app.core.ultimate_use_pushover = true;
    let run = app
        .core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .pushover_x
        .as_mut()
        .unwrap();
    let step = run.confirmed_history.as_ref().unwrap()[1].step;
    run.capacity_evaluation = Some(
        run.evaluation_point(
            EvaluationPurpose::HoldingCapacity,
            SeismicDir::X,
            step,
            "RC柱の終局せん断の採用ステップ".into(),
        )
        .unwrap(),
    );
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let saved = load_ovika(&path).unwrap();
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
    assert!(loaded.core.ultimate_use_pushover);
    let labels = ultimate_table_labels(&mut loaded);
    for number in ["301.9", "7.60"] {
        assert!(labels.iter().any(|label| label == number), "{labels:?}");
    }

    let mut saved_results: SavedResults = rmp_serde::from_slice(&saved.results.unwrap()).unwrap();
    assert_eq!(
        saved_results.bundle.analysis_contract_version,
        Some(ANALYSIS_CONTRACT_VERSION)
    );
    // 廃止直前の版の境界を検証するため、入力照合に必要な現行の記録をそろえたまま版を戻す。
    saved_results.bundle.analysis_contract_version = Some(108);
    saved_results.bundle.design_contract_version = Some(109);
    let old_results = rmp_serde::to_vec_named(&saved_results).unwrap();

    let mut settings: serde_json::Value =
        rmp_serde::from_slice(&saved.analysis_settings.unwrap()).unwrap();
    let retired = [
        ("ultimate_shear_ductility", serde_json::json!(true)),
        ("ultimate_rp", serde_json::json!(0.05)),
        ("ultimate_lightweight", serde_json::json!(true)),
    ];
    for (field, value) in &retired {
        assert!(settings["design_settings"].get(field).is_none());
        settings["design_settings"][field] = value.clone();
    }
    let old_settings = rmp_serde::to_vec_named(&settings).unwrap();
    save_ovika(
        &path,
        &saved.model,
        OvikaExtras {
            results: Some(&old_results),
            analysis_settings: Some(&old_settings),
            ..Default::default()
        },
    )
    .unwrap();
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert!(
        reopened.core.scoped.last_error.is_none(),
        "{:?}",
        reopened.core.scoped.last_error
    );
    assert_eq!(reopened.core.model.elements.len(), 1);
    assert!(!reopened.core.ultimate_use_pushover);
    assert!(reopened.core.log.entries.iter().any(|entry| {
        entry
            .message
            .contains("保存された解析タブの設定値を読み込めませんでした")
            && entry.message.contains("ultimate_")
    }));
    assert!(reopened.pushover_for(SeismicDir::X).is_none());
    let notice = reopened
        .core
        .scoped
        .result_contract_notice
        .as_deref()
        .unwrap();
    assert!(
        notice.contains("保存版=108") && notice.contains("再実行"),
        "{notice}"
    );
    reopened.core.ultimate_use_pushover = true;
    let error = reopened.compute_ultimate_checks().unwrap_err();
    assert!(error.contains("ultimate_input_stale"), "{error}");
    let labels = ultimate_table_labels(&mut reopened);
    let text = labels.join("\n");
    for expected in [
        "終局せん断は適用条件を満たす普通コンクリートの無孔矩形断面が対象",
        "梁は荒川mean式、柱は修正荒川mean式",
        "円形柱は曲げ・軸の照査対象ですが、終局せん断は未算定",
        "RC規準16条",
        "確定総応答が不足・未検定",
        "増分解析を再実行",
    ] {
        assert!(text.contains(expected), "{text}");
    }
    for number in ["301.9", "7.60"] {
        assert!(!labels.iter().any(|label| label == number), "{labels:?}");
    }
    for retired_label in ["靭性指針式(Vu)", "塑性理論式(Qsu)", "ヒンジ回転角 Rp:"] {
        assert!(!text.contains(retired_label), "{text}");
    }
    reopened.save_project_to(path.clone());
    assert!(
        reopened.core.scoped.last_error.is_none(),
        "{:?}",
        reopened.core.scoped.last_error
    );
    let resaved = load_ovika(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(resaved.results.is_none());
    let settings: serde_json::Value =
        rmp_serde::from_slice(&resaved.analysis_settings.unwrap()).unwrap();
    assert_eq!(settings["design_settings"]["ultimate_use_pushover"], true);
    for (field, _) in retired {
        assert!(
            settings["design_settings"].get(field).is_none(),
            "{settings}"
        );
    }
}

#[test]
fn actual_rc_beam_shear_uses_a_valid_physical_span_even_if_legacy_span_is_negative() {
    use sepika_core::dof::{Dof, Dof6Mask};
    use sepika_core::model::{ForceRegime, PanelZoneMode, RcBeamReference, RigidZone, ZoneSource};
    use sepika_core::section_shape::{RebarSize, StandardRebar};

    let mut model = beam_model(&[3], &[3]);
    model.panel_zone = PanelZoneMode::None;
    model.nodes[3].coord[0] = 1_500.0;
    model.nodes[3].restraint = Dof6Mask::FIXED;
    model.nodes[3].restraint.set_free(Dof::Uz);
    let SectionShape::RcBeamRect { rebar, .. } = model.sections[1].shape.clone().unwrap() else {
        unreachable!()
    };
    let mut rebar = rebar;
    rebar.main_dia = RebarSize::Standard(StandardRebar::D19);
    rebar.stirrup.dia = RebarSize::Standard(StandardRebar::D10);
    let mut section = SectionShape::RcBeamRect {
        b: 250.0,
        d: 350.0,
        rebar,
    }
    .to_section(SectionId(1), "検定梁".into());
    section.frame_use = Some(FrameSectionUse::Girder);
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = Some(MaterialId(1));
    model.sections[1] = section;
    model.elements[1].force_regime = ForceRegime::UniaxialBendingShear;
    model.set_member_rc_beam_reference(
        ElemId(1),
        Some(RcBeamReference::AntisymmetricHalfMemberWithoutAxialForce),
    );

    let mut support = model.sections[0].clone();
    let SectionShape::RcColumnRect { rebar, .. } = support.shape.take().unwrap() else {
        unreachable!()
    };
    let mut rebar = rebar;
    rebar.x = vec![4];
    rebar.y = vec![10];
    let mut section = SectionShape::RcColumnRect {
        b: 400.0,
        d: 2_000.0,
        rebar,
    }
    .to_section(SectionId(2), "幅側で支持する柱".into());
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = support.material;
    section.rebar_material = support.rebar_material;
    section.shear_rebar_material = support.shear_rebar_material;
    model.sections.push(section);
    for i in 0..2_u32 {
        let mut base = model.nodes[2 + i as usize].clone();
        base.id = NodeId(4 + i);
        base.coord[2] = 0.0;
        model.nodes.push(base);
        let mut column = model.elements[0].clone();
        column.id = ElemId(2 + i);
        column.nodes = [NodeId(4 + i), NodeId(2 + i)].into_iter().collect();
        column.section = Some(SectionId(2));
        column.local_axis.ref_vector = [0.0, 1.0, 0.0];
        model.elements.push(column);
    }
    for elem in &mut model.elements {
        elem.rigid_zone = RigidZone {
            source_i: ZoneSource::Manual,
            source_j: ZoneSource::Manual,
            ..Default::default()
        };
    }
    model.load_cases[0].nodal.push(NodalLoad::manual(
        NodeId(3),
        [0.0, 0.0, -10_000.0, 0.0, 0.0, 0.0],
    ));
    let mut app = solve_beam_model(model);
    app.generate_stories_action();
    app.run_seismic(SeismicDir::Y);
    app.core.analysis_cfg.push_dir = SeismicDir::Y;
    app.core.analysis_cfg.push_steps = 3;
    app.run_pushover();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app.core.ultimate_use_pushover = true;
    let po = app
        .core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .pushover_y
        .as_mut()
        .unwrap();
    let response = &po.confirmed_history.as_ref().unwrap()[1];
    let actions = response
        .members
        .iter()
        .find(|member| member.elem == ElemId(1))
        .unwrap()
        .rc_beam_ultimate_shear
        .unwrap()
        .unwrap();
    assert_eq!(actions.axial_range_n, [0.0; 2]);
    assert_eq!(actions.max_mq[1], (0.0, 0.0));
    // 等断面・両端回転拘束で両端の実柱面まで200 mm。材端から200 mmの最外曲げはQ×550 mm。
    close(actions.max_mq[0].0, 5_500_000.0);
    close(actions.max_mq[0].1, 10_000.0);
    po.capacity_evaluation = Some(
        po.evaluation_point(
            EvaluationPurpose::HoldingCapacity,
            SeismicDir::Y,
            response.step,
            "実柱面間の単軸応答".into(),
        )
        .unwrap(),
    );

    let model = &app.core.model;
    assert!(
        sepika_design_jp::rc::rc_member_clear_span(&model.elements[1], model)
            .unwrap_err()
            .to_string()
            .contains("正の内法長さが成立しません")
    );
    let checks = app.compute_ultimate_checks().unwrap();
    let beam = checks.iter().find(|check| check.elem == ElemId(1)).unwrap();
    assert!(beam.mu.is_nan() && beam.bond_stress.is_none(), "{beam:?}");
    assert!(beam
        .unchecked_reason
        .as_deref()
        .unwrap()
        .contains("ultimate_clear_span_invalid"));
    close(beam.qmu, 10_000.0);
    close(beam.qsu, 163_789.083_862_271);
    close(beam.shear_margin, 16.378_908_386_227);

    let saved = app
        .core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .pushover_y
        .as_mut()
        .unwrap();
    let selected = &mut saved.confirmed_history.as_mut().unwrap()[1];
    selected
        .members
        .iter_mut()
        .find(|member| member.elem == ElemId(1))
        .unwrap()
        .rc_beam_ultimate_shear = None;
    let checks = app.compute_ultimate_checks().unwrap();
    let beam = checks.iter().find(|check| check.elem == ElemId(1)).unwrap();
    assert!(beam.qsu.is_nan() && beam.shear_margin.is_nan(), "{beam:?}");
}

#[test]
fn actual_rc_cantilever_with_member_load_displays_capacity_and_reference_mu() {
    use sepika_core::dof::Dof;
    use sepika_core::model::{ForceRegime, MemberLoad, MemberLoadKind};

    let mut model = beam_model(&[4], &[4]);
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut model.sections[1].shape else {
        unreachable!()
    };
    rebar.main_dia = sepika_core::section_shape::RebarSize::Standard(
        sepika_core::section_shape::StandardRebar::D22,
    );
    for dof in [Dof::Uy, Dof::Rx, Dof::Rz] {
        model.nodes[3].restraint.set_fixed(dof);
    }
    model.elements[1].force_regime = ForceRegime::AxialBendingInteract;
    model.load_cases[0].nodal.push(NodalLoad::manual(
        NodeId(3),
        [0.0, 0.0, -10_000.0, 0.0, 0.0, 0.0],
    ));
    model.load_cases[0].member.push(MemberLoad::manual(
        ElemId(1),
        [0.0, 0.0, -1.0],
        MemberLoadKind::Distributed {
            a: 0.0,
            b: 6000.0,
            w1: 0.5,
            w2: 0.5,
        },
    ));
    let mut app = solve_beam_model(model);
    app.generate_stories_action();
    app.run_seismic(SeismicDir::Y);
    app.core.analysis_cfg.push_dir = SeismicDir::Y;
    app.core.analysis_cfg.push_steps = 3;
    app.run_pushover();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app.core.ultimate_use_pushover = true;
    let po = app
        .core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .pushover_y
        .as_mut()
        .unwrap();
    let response = &po.confirmed_history.as_ref().unwrap()[1];
    let member = response
        .members
        .iter()
        .find(|member| member.elem == ElemId(1))
        .unwrap();
    let actions = member.rc_beam_ultimate_shear.unwrap().unwrap();
    close(actions.max_mq[0].0, 69e6);
    close(actions.max_mq[0].1, 13e3);
    po.capacity_evaluation = Some(
        po.evaluation_point(
            EvaluationPurpose::HoldingCapacity,
            SeismicDir::Y,
            response.step,
            "RC梁の総せん断力を照合するstep".into(),
        )
        .unwrap(),
    );
    let checks = app.compute_ultimate_checks().unwrap();
    let beam = checks.iter().find(|check| check.elem == ElemId(1)).unwrap();
    close(beam.qmu, 13e3);
    close(beam.qsu, 332_617.813_860_871_25);
    close(beam.shear_margin, 25.585_985_681_605_48);
    assert!(
        beam.detail.contains("参考曲げ略算 Mu")
            && beam
                .detail
                .contains("ultimate_beam_mu_applicability_unverified:")
    );
    #[cfg(feature = "gui")]
    {
        let labels = ultimate_table_labels(&mut app);
        assert!(
            labels.iter().any(|label| label.ends_with("（参考）")),
            "{labels:?}"
        );
        assert!(labels.iter().any(|label| label == "未検定"), "{labels:?}");
        assert!(labels.iter().any(|label| label == "未算定"), "{labels:?}");
    }
}

#[test]
fn actual_rc_fixed_beam_reports_an_unconfirmed_mechanism_or_released_end() {
    use sepika_core::{dof::Dof6Mask, model::EndCondition};

    for (condition, expected) in [
        (
            EndCondition::Fixed,
            "ultimate_beam_hinge_mechanism_unverified:",
        ),
        (
            EndCondition::Pinned,
            "ultimate_beam_hinge_end_released: j端",
        ),
    ] {
        let mut model = beam_model(&[2], &[2]);
        model.nodes[3].restraint = Dof6Mask::FIXED;
        model.elements[1].end_cond[1] = condition;
        let mut app = solve_beam_model(model);
        let checks = app.compute_ultimate_checks().unwrap();
        let beam = checks.iter().find(|check| check.elem == ElemId(1)).unwrap();
        assert!(!beam.ok && beam.qmu.is_nan());
        let reason = beam.unchecked_reason.as_deref().unwrap();
        assert!(reason.contains(expected), "{reason}");
    }
}
