use super::*;
use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::model::{ForceRegime, GirderTorsionMode, MemberLoad, MemberLoadKind, Model};
use sepika_core::rc_capacity::RcBeamUltimateShearIssue as Issue;
use sepika_core::section_shape::{RebarSize, StandardRebar};
use sepika_design_jp::ultimate::UltimateCheck;
use sepika_solver::nonlinear::pushover::{PushoverMemberResponse, PushoverResult};

fn model(splits_mm: &[f64], grouped: bool, distributed: bool) -> Model {
    let mut model = beam_model(&[4], &[4]);
    model.girder_torsion = GirderTorsionMode::Keep;
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut model.sections[1].shape else {
        unreachable!()
    };
    rebar.main_dia = RebarSize::Standard(StandardRebar::D22);
    rebar.stirrup.dia = RebarSize::Standard(StandardRebar::D10);
    model.nodes[3].coord[0] = 4_000.0;
    for index in [2, 3] {
        model.nodes[index].restraint = Dof6Mask::FIXED;
        model.nodes[index].restraint.set_free(Dof::Ry);
    }
    model.nodes[3].restraint.set_free(Dof::Ux);

    let template = model.elements.pop().unwrap();
    let mut ends = vec![(0.0, NodeId(2))];
    for &x in splits_mm {
        let mut node = model.nodes[3].clone();
        node.id = NodeId(model.nodes.len() as u32);
        node.coord[0] = x;
        node.restraint.set_free(Dof::Uz);
        ends.push((x, node.id));
        model.nodes.push(node);
    }
    ends.push((4_000.0, NodeId(3)));
    let group: Vec<_> = ends
        .windows(2)
        .enumerate()
        .map(|(part, ends)| {
            let mut elem = template.clone();
            let id = ElemId(part as u32 + 1);
            elem.id = id;
            elem.nodes = [ends[0].1, ends[1].1].into_iter().collect();
            elem.force_regime = ForceRegime::AxialBendingInteract;
            model.elements.push(elem);
            id
        })
        .collect();

    if grouped {
        model.girder_groups.push(group.clone());
    }
    let case = &mut model.load_cases[0];
    if distributed {
        for (id, points) in group.into_iter().zip(ends.windows(2)) {
            case.member.push(MemberLoad::manual(
                id,
                [0.0, 0.0, -1.0],
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: points[1].0 - points[0].0,
                    w1: 1.0,
                    w2: 1.0,
                },
            ));
        }
    } else {
        for x in [1_000.0, 2_000.0, 3_000.0] {
            if let Some(&(_, node)) = ends.iter().find(|(p, _)| *p == x) {
                case.nodal.push(NodalLoad::manual(
                    node,
                    [0.0, 0.0, -40_000.0, 0.0, 0.0, 0.0],
                ));
            } else {
                let part = ends.partition_point(|(p, _)| *p < x) - 1;
                case.member.push(MemberLoad::manual(
                    group[part],
                    [0.0, 0.0, -1.0],
                    MemberLoadKind::Point {
                        a: x - ends[part].0,
                        p: 40_000.0,
                    },
                ));
            }
        }
    }
    model
}

fn stored_run(app: &mut App) -> &mut PushoverResult {
    app.core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .pushover_y
        .as_mut()
        .unwrap()
}

fn adopted_beam_rows(app: &mut App) -> &mut [PushoverMemberResponse] {
    let run = stored_run(app);
    let step = run.capacity_evaluation.as_ref().unwrap().step as usize;
    &mut run.confirmed_history.as_mut().unwrap()[step].members
}

fn pushed(mut model: Model) -> App {
    // 梁は、増分載荷方向の変位を独立した柱だけで制御する。
    model.nodes[1].restraint.set_fixed(Dof::Ux);
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
    app
}

fn adopted(model: Model) -> App {
    let mut app = pushed(model);
    let run = stored_run(&mut app);
    assert!(run.steps.len() > 2);
    run.capacity_evaluation = Some(
        run.evaluation_point(
            EvaluationPurpose::HoldingCapacity,
            SeismicDir::Y,
            1,
            "一本部材の終局せん断".into(),
        )
        .unwrap(),
    );
    app.core.ultimate_use_pushover = true;
    app.core.ultimate_include_bond = false;
    app
}

fn assert_beam(app: &mut App, count: usize, [m, q, capacity, margin]: [f64; 4]) {
    let demands = app.ultimate_demand_by_elem().unwrap();
    let mut actions = demands
        .iter()
        .filter_map(|(id, demand)| (id.0 != 0).then_some(demand.rc_beam_ultimate_shear));
    let expected = actions.next().unwrap().unwrap().unwrap();
    assert_eq!(expected.axial_range_n, [0.0; 2]);
    close(expected.max_mq[0].0, m);
    close(expected.max_mq[0].1, q);
    assert!(actions.all(|actions| actions == Some(Ok(expected))));
    let checks = app.compute_ultimate_checks().unwrap();
    assert_eq!(checks.len(), 1 + count);
    for row in checks.iter().filter(|row| row.elem != ElemId(0)) {
        assert!(row.ok, "{row:?}");
        close(row.qmu, q);
        close(row.qsu, capacity);
        close(row.shear_margin, margin);
    }
}

fn reopen(app: &mut App, label: &str) -> App {
    let path =
        std::env::temp_dir().join(format!("sepika1889-{label}-{}.ovika", std::process::id()));
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
    loaded
}

#[test]
fn arbitrary_divisions_and_nodal_or_member_point_loads_keep_whole_beam_shear() {
    // 左右反力60 kN、区間せん断60, 20, -20, -60 kN。最大曲げ80 kN·mは中央。
    let expected = [80e6, 60e3, 356_976.084_710_482_3, 5.949_601_411_84];
    for splits in [&[1_100.0, 2_500.0][..], &[1_000.0, 2_000.0, 3_000.0]] {
        let mut input = model(splits, true, false);
        let mut section = input.sections[1].clone();
        section.id = SectionId(2);
        section.name = "同じ配筋の別符号".into();
        input.sections.push(section);
        input.elements[2].section = Some(SectionId(2));
        let mut app = adopted(input);
        assert_beam(&mut app, splits.len() + 1, expected);
        if splits.len() != 3 {
            continue;
        }
        #[cfg(feature = "gui")]
        {
            let labels = ultimate_table_labels(&mut app);
            for value in ["357.0", "5.95"] {
                assert_eq!(
                    labels.iter().filter(|v| v.as_str() == value).count(),
                    4,
                    "{labels:?}"
                );
            }
        }

        let mut loaded = reopen(&mut app, "whole");
        assert_beam(&mut loaded, 4, expected);
    }
}

#[test]
fn whole_beam_recovery_finds_the_curve_peak_under_uniform_load() {
    // 全長4 m、全区間荷重1 N/mm。左右反力2 kN、中央曲げ2 kN·m。
    for splits in [&[][..], &[1_500.0, 2_750.0]] {
        let mut app = adopted(model(splits, !splits.is_empty(), true));
        assert_beam(
            &mut app,
            splits.len() + 1,
            [2e6, 2e3, 417_243.980_291_087_94, 208.621_990_145_54],
        );
    }
}

fn assert_unchecked(app: &mut App, reason: &str) {
    assert_unchecked_rows(&app.compute_ultimate_checks().unwrap(), reason);
}

fn assert_unchecked_rows(checks: &[UltimateCheck], reason: &str) {
    for row in checks.iter().filter(|row| row.elem != ElemId(0)) {
        assert!(
            !row.ok && row.qsu.is_nan() && row.shear_margin.is_nan(),
            "{row:?}"
        );
        assert!(
            row.unchecked_reason
                .as_ref()
                .is_some_and(|r| r.contains(reason)),
            "{row:?}"
        );
    }
}

fn reference_checks(app: &mut App) -> Vec<UltimateCheck> {
    use sepika_design_jp::ultimate::{collect_rc_ultimate_checks, UltimateShearOptions};
    let members = &stored_run(app).confirmed_history.as_ref().unwrap()[1].members;
    let demand = sepika_job::member_demand_from_pushover(members, None).unwrap();
    collect_rc_ultimate_checks(
        &app.core.model,
        &demand,
        &UltimateShearOptions {
            include_bond: false,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn a_nonuniform_section_or_mismatched_local_axes_rejects_every_display_row() {
    let mut input = model(&[2_000.0], true, false);
    input.elements[2].local_axis.ref_vector = [0.0, 0.0, -1.0];
    assert_unchecked(&mut adopted(input), Issue::LocalAxis.reason());
    let mut input = model(&[2_000.0], true, false);
    let mut section = input.sections[1].clone();
    section.id = SectionId(2);
    section.name = "違う配筋".into();
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut section.shape else {
        unreachable!()
    };
    rebar.top = vec![3];
    rebar.bottom = vec![3];
    input.sections.push(section);
    input.elements[2].section = Some(SectionId(2));
    assert_unchecked(&mut adopted(input), Issue::Section.reason());
}

#[test]
fn old_or_inconsistent_group_records_do_not_fall_back_to_section_actions() {
    let mut app = adopted(model(&[2_000.0], true, false));
    for row in adopted_beam_rows(&mut app)
        .iter_mut()
        .filter(|row| row.elem != ElemId(0))
    {
        row.rc_ultimate_shear = row.rc_beam_ultimate_shear.unwrap().ok();
    }
    for id in [ElemId(1), ElemId(2)] {
        let row = adopted_beam_rows(&mut app)
            .iter_mut()
            .find(|row| row.elem == id)
            .unwrap();
        row.rc_beam_ultimate_shear = None;
        assert_unchecked(&mut app, "増分解析を再実行してください");
    }
    assert_unchecked(
        &mut reopen(&mut app, "legacy"),
        "増分解析を再実行してください",
    );

    let run = stored_run(&mut app);
    let step = run.capacity_evaluation.as_ref().unwrap().step as usize;
    run.confirmed_history.as_mut().unwrap()[step]
        .members
        .retain(|row| row.elem != ElemId(1));
    assert!(app
        .compute_ultimate_checks()
        .unwrap_err()
        .contains("ultimate_response_missing:"));
}

#[test]
fn supported_internal_node_and_axial_force_prevent_whole_beam_comparison() {
    let mut input = model(&[2_000.0], true, false);
    input.nodes[4].restraint.set_fixed(Dof::Uz);
    assert_unchecked_rows(
        &reference_checks(&mut pushed(input)),
        Issue::InteriorSupport.reason(),
    );
    let mut input = model(&[2_000.0], true, false);
    input.load_cases[0].nodal.push(NodalLoad::manual(
        NodeId(3),
        [-10_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    ));
    let mut app = pushed(input);
    assert!(stored_run(&mut app)
        .shear_design_unavailable_at(1)
        .is_some());
    assert_unchecked_rows(&reference_checks(&mut app), "梁は軸力なし");
}

#[test]
fn a_weak_axis_member_load_never_becomes_a_verified_uniaxial_beam() {
    let mut input = model(&[2_000.0], true, false);
    input.load_cases[0].member.push(MemberLoad::manual(
        ElemId(1),
        [0.0, 1.0, 0.0],
        MemberLoadKind::Point { a: 500.0, p: 500.0 },
    ));
    let mut app = pushed(input);
    assert!(stored_run(&mut app)
        .shear_design_unavailable_at(1)
        .is_some());
    for row in reference_checks(&mut app)
        .iter()
        .filter(|row| row.elem != ElemId(0))
    {
        assert!(!row.ok);
        assert!(
            row.unchecked_reason
                .as_ref()
                .is_some_and(|r| r.contains("RC梁の弱軸曲げ・せん断")),
            "{row:?}"
        );
    }
}
