#[path = "../../sepika-core/tests/support/secondary_materialization_model.rs"]
mod fixture;
use sepika_core::{ids::*, model::*};

#[test]
fn independent_beam_keeps_self_weight_transfer_and_design_target() {
    for (primary, secondary, expected_count) in [
        (-300.0, 0.0, 1),
        (-300.0, -300.0, 1),
        (0.0, -300.0, 0),
        (0.0, 0.0, 0),
    ] {
        let mut model = fixture::model();
        model.elements[0].end_offsets_mm = [[0.0, 0.0, primary]; 2];
        model.unassigned_beams[0].end_offsets_mm = [[0.0, 0.0, secondary]; 2];
        let transfer = sepika_load::cascade::solve(&model, |_| 0., true).unwrap();
        assert_eq!(transfer.members.len(), expected_count);
        assert!(transfer.unresolved.is_empty());
        assert!(transfer.cyclic.is_empty());
        let gaps = sepika_load::floor::secondary_beam_distribution_gaps(&model).unwrap();
        assert_eq!(gaps.no_distribution, expected_count);
        let target = sepika_design_jp::floor::secondary_beam_steel_e_and_ft(
            &model,
            &model.unassigned_beams[0],
        )
        .unwrap();
        assert_eq!(usize::from(target.is_some()), expected_count);
        if expected_count == 1 {
            let beam = &transfer.members[&SecondaryMemberId(0)];
            // H400の矩形板3枚: A=2*200*13+(400-26)*8=8192 mm²、γ=78.5e-6 N/mm³、支持梁幅200 mmずつの半幅を控除して5800 mm。
            let expected_weight_n = 8192. * 5800. * 78.5e-6;
            for reaction in beam.reactions {
                assert!(
                    (reaction - expected_weight_n / 2.).abs() < 1e-6,
                    "{reaction}"
                );
            }
            let (nodal, member) = transfer.primary_loads(&model);
            assert!(member.is_empty());
            assert_eq!(nodal.len(), 2);
            assert!((nodal.iter().map(|(_, p)| p).sum::<f64>() - expected_weight_n).abs() < 1e-6);
        } else {
            let (nodal, member) = transfer.primary_loads(&model);
            assert!(nodal.is_empty() && member.is_empty());
        }
    }
}

#[test]
fn precheck_keeps_free_end_warning_for_distinct_eccentricity() {
    let mut model = fixture::model();
    model.unassigned_beams[0].ends = SecondaryMemberEnds::Cantilever {
        support: SecondaryMemberAnchor {
            support: SupportMemberId::Primary(ElemId(1)),
            position: 0.,
        },
        free_end_vector: [6000., 0.],
    };
    let warnings = |model: &Model| {
        sepika_solver::statics::analysis::precheck::model_issues(model)
            .into_iter()
            .filter(|issue| issue.message.contains("幾何的には支持がある端"))
            .count()
    };
    assert_eq!(warnings(&model), 1);
    model.elements[0].end_offsets_mm = [[0.0, 0.0, 0.]; 2];
    assert_eq!(warnings(&model), 0);
}

#[test]
fn precheck_reports_unresolved_independent_beam_endpoints() {
    let mut model = fixture::model();
    model.unassigned_beams[0].ends =
        SecondaryMemberEnds::Detached([[0., 0., 3000.], [6000., 0., 3000.]]);
    let issues = sepika_solver::statics::analysis::precheck::model_issues(&model);
    assert!(issues.iter().any(|issue| issue.severity
        == sepika_solver::statics::analysis::precheck::IssueSeverity::Error
        && issue
            .message
            .contains("支持部材アンカーへ解決できない二次部材が 1 本")));
}

#[test]
fn multistage_secondary_display_offsets_leave_transfer_weight_quantity_and_design_unchanged() {
    let mut model = fixture::model();
    model.unassigned_beams.push(SecondaryMember {
        id: SecondaryMemberId(1),
        kind: SecondaryMemberKind::Beam,
        ends: SecondaryMemberEnds::Supported([
            SecondaryMemberAnchor {
                support: SupportMemberId::Secondary(SecondaryMemberId(0)),
                position: 0.5,
            },
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(1)),
                position: 1.0,
            },
        ]),
        section: Some(SectionId(0)),
        ..Default::default()
    });
    let design = |model: &Model, transfer: &sepika_load::cascade::SecondaryTransfer| {
        model
            .beams()
            .filter_map(|sm| {
                let (section, young, ft) =
                    sepika_design_jp::floor::secondary_beam_steel_e_and_ft(model, sm).unwrap()?;
                let entry = transfer.members.get(&sm.id)?;
                let demand = sepika_load::floor::simple_beam_extremes(
                    &entry.member_loads,
                    entry.span,
                    young,
                    section.iy,
                )
                .unwrap();
                let result = sepika_design_jp::floor::design_beam_from_forces(
                    entry.span,
                    demand.w_equiv,
                    demand.m_max,
                    demand.q_max,
                    demand.deflection,
                    section.iy / (section.depth / 2.0),
                    ft,
                    sepika_design_jp::floor::DEFLECTION_LIMIT_DENOM,
                )
                .unwrap();
                Some((sm.id, result))
            })
            .collect::<Vec<_>>()
    };
    for materialized in [false, true] {
        model.elements[0].end_offsets_mm = [[0.0, 0.0, if materialized { 0.0 } else { -300.0 }]; 2];
        let baseline_transfer = sepika_load::cascade::solve(&model, |_| 0.003, true).unwrap();
        let baseline_quantity = sepika_design_jp::quantity::try_compute_quantity_takeoff(
            &model,
            &QuantityCfg::default(),
        )
        .unwrap();
        let baseline_design = design(&model, &baseline_transfer);
        let baseline_input = model.calculation_model();
        let baseline_weight_input =
            sepika_job::weight_preparation::weight_input_key(&model, model.mass_method);
        for sm in &mut model.unassigned_beams {
            sm.end_offsets_mm = [[100.0, 200.0, 300.0], [-400.0, 500.0, -600.0]];
        }
        let transfer = sepika_load::cascade::solve(&model, |_| 0.003, true).unwrap();
        assert_eq!(baseline_transfer.members.len(), transfer.members.len());
        for (id, expected) in &baseline_transfer.members {
            let actual = &transfer.members[id];
            assert_eq!(actual.end_points, expected.end_points);
            assert_eq!(actual.span, expected.span);
            assert_eq!(actual.reactions, expected.reactions);
            assert_eq!(actual.supports, expected.supports);
            assert_eq!(actual.member_loads, expected.member_loads);
        }
        let quantity = sepika_design_jp::quantity::try_compute_quantity_takeoff(
            &model,
            &QuantityCfg::default(),
        )
        .unwrap();
        assert_eq!(baseline_quantity.items.len(), quantity.items.len());
        for (expected, actual) in baseline_quantity.items.iter().zip(&quantity.items) {
            assert_eq!(actual.elem, expected.elem);
            assert_eq!(actual.secondary, expected.secondary);
            assert_eq!(actual.concrete_m3, expected.concrete_m3);
            assert_eq!(actual.formwork_m2, expected.formwork_m2);
            assert_eq!(actual.steel_weight_t(), expected.steel_weight_t());
            assert_eq!(actual.rebar_weight_t(), expected.rebar_weight_t());
        }
        assert_eq!(baseline_design, design(&model, &transfer));
        assert!(model
            .calculation_model()
            .eq_ignoring_dofmap(&baseline_input));
        assert_eq!(
            sepika_job::weight_preparation::weight_input_key(&model, model.mass_method),
            baseline_weight_input
        );
        for sm in &mut model.unassigned_beams {
            sm.end_offsets_mm = [[0.0; 3]; 2];
        }
    }
}

#[path = "../../sepika-core/tests/support/steel_secondary_post_model.rs"]
mod post_fixture;

#[test]
fn post_xyz_display_offsets_preserve_gravity_local_lateral_design_quantity_and_mass_input() {
    let baseline = post_fixture::steel_post_model();
    let mut shifted = baseline.clone();
    shifted.wall_regions[0].posts[0].end_offsets_mm =
        [[100.0, 200.0, 300.0], [-400.0, 500.0, -600.0]];
    let original = sepika_load::cascade::solve(&baseline, |_| 0.003, true).unwrap();
    let actual = sepika_load::cascade::solve(&shifted, |_| 0.003, true).unwrap();
    let id = SecondaryMemberId(0);
    assert_eq!(
        original.members[&id].end_points,
        actual.members[&id].end_points
    );
    assert_eq!(original.members[&id].span, 3000.0);
    assert_eq!(
        original.members[&id].member_loads,
        actual.members[&id].member_loads
    );
    assert_eq!(
        original.members[&id].reactions,
        actual.members[&id].reactions
    );
    let (expected_nodes, expected_members) = original.primary_loads(&baseline);
    let (actual_nodes, actual_members) = actual.primary_loads(&shifted);
    assert_eq!(actual_nodes, expected_nodes);
    let loads = |v: &[sepika_load::floor::BeamLoad]| {
        v.iter()
            .map(|l| format!("{:?}", (l.elem, l.target, l.shape, l.cmq)))
            .collect::<Vec<_>>()
    };
    assert_eq!(loads(&actual_members), loads(&expected_members));
    let expected = sepika_design_jp::post::design_post(&baseline, baseline.posts().next().unwrap());
    let result = sepika_design_jp::post::design_post(&shifted, shifted.posts().next().unwrap());
    assert!(expected.complete(), "{expected:?}");
    assert_eq!(result, expected);
    assert!(result.combinations[0]
        .lateral_reactions
        .as_ref()
        .unwrap()
        .iter()
        .flatten()
        .any(|v| *v != 0.0));
    assert!(baseline
        .calculation_model()
        .eq_ignoring_dofmap(&shifted.calculation_model()));
    assert_eq!(
        sepika_job::weight_preparation::weight_input_key(&baseline, baseline.mass_method),
        sepika_job::weight_preparation::weight_input_key(&shifted, shifted.mass_method)
    );
    let quantity = |m: &Model| {
        sepika_design_jp::quantity::try_compute_quantity_takeoff(m, &QuantityCfg::default())
            .unwrap()
    };
    let expected = quantity(&baseline);
    let actual = quantity(&shifted);
    assert_eq!(expected.items.len(), actual.items.len());
    for (expected, actual) in expected.items.iter().zip(&actual.items) {
        assert_eq!(actual.elem, expected.elem);
        assert_eq!(actual.secondary, expected.secondary);
        assert_eq!(actual.concrete_m3, expected.concrete_m3);
        assert_eq!(actual.formwork_m2, expected.formwork_m2);
        assert_eq!(actual.steel_weight_t(), expected.steel_weight_t());
        assert_eq!(actual.rebar_weight_t(), expected.rebar_weight_t());
    }
}

#[test]
fn column_static_response_keeps_physical_axial_force_and_transfers_support_moment_once() {
    let source = post_fixture::steel_post_model();
    let mut model = Model::with_default_load_cases();
    model.materials = source.materials.clone();
    model.sections.push(source.sections[0].clone());
    let mut bottom = source.nodes[0].clone();
    bottom.coord = [0.0, 0.0, 0.0];
    let mut top = source.nodes[2].clone();
    top.id = NodeId(1);
    top.coord = [0.0, 0.0, 3000.0];
    top.restraint = Default::default();
    model.nodes = vec![bottom, top];
    let mut column = source.elements[2].clone();
    column.id = ElemId(0);
    column.nodes = vec![NodeId(0), NodeId(1)].into();
    column.section = Some(SectionId(0));
    column.end_offsets_mm = [[100.0, 0.0, 0.0]; 2];
    model.elements.push(column);
    model.load_cases[0].nodal.push(NodalLoad {
        node: NodeId(1),
        values: [0.0, 0.0, -50000.0, 0.0, 5000000.0, 0.0],
        name: "実端圧縮荷重".into(),
        source: LoadSource::Manual,
    });
    let result = sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(0)).unwrap();
    for (_, f) in &result.member_forces[0].1.at {
        assert!((f[0] + 50000.0).abs() < 1e-7, "{f:?}");
        assert!(f[3..].iter().all(|m| m.abs() < 1e-6), "{f:?}");
    }
    let behavior = sepika_element::factory::build_behavior(&model.elements[0], &model);
    let disp: Vec<_> = result.disp.iter().flatten().copied().collect();
    let response = behavior.frame_load_response(&disp, &[]).unwrap().unwrap();
    let base = &response.nodal_residual.data[..6];
    assert!((base[2] - 50000.0).abs() < 1e-7);
    assert!((base[4] + 5000000.0).abs() < 1e-6);
    assert!((base[2] + model.load_cases[0].nodal[0].values[2]).abs() < 1e-7);
    assert!((base[4] + model.load_cases[0].nodal[0].values[4]).abs() < 1e-6);
    for (offsets, length_mm, centroid_x_mm, physical_base_bending_arm_mm) in [
        ([[100.0, 0.0, 0.0]; 2], 3000.0, 100.0, 0.0),
        (
            [[100.0, 0.0, 0.0], [500.0, 0.0, 0.0]],
            3026.549190084311,
            300.0,
            200.0,
        ),
    ] {
        model.elements[0].end_offsets_mm = offsets;
        let cfg = LoadCfg {
            steel_weight_factor: 1.0,
            ..Default::default()
        };
        let (nodal, member) =
            sepika_load::self_weight::self_weight_case_content(&model, &cfg).unwrap();
        let weight_n = 4544.0 * 78.5e-6 * length_mm;
        if physical_base_bending_arm_mm == 0.0 {
            assert!(member.is_empty());
            assert!((nodal.iter().map(|load| load.values[2]).sum::<f64>() + weight_n).abs() < 1e-8);
            assert!(
                (nodal.iter().map(|load| load.values[4]).sum::<f64>() - 100.0 * weight_n).abs()
                    < 1e-7
            );
        } else {
            assert!(nodal.is_empty());
            assert_eq!(member.len(), 1);
        }
        model.load_cases[0].nodal = nodal;
        model.load_cases[0].member = member;
        let result =
            sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(0)).unwrap();
        let behavior = sepika_element::factory::build_behavior(&model.elements[0], &model);
        let disp: Vec<_> = result.disp.iter().flatten().copied().collect();
        let response = behavior
            .frame_load_response(&disp, &model.load_cases[0].member)
            .unwrap()
            .unwrap();
        let mut base: [f64; 6] = response.nodal_residual.data[..6].try_into().unwrap();
        for load in &model.load_cases[0].nodal {
            if load.node == NodeId(0) {
                for (reaction, force) in base.iter_mut().zip(load.values) {
                    *reaction -= force;
                }
            }
        }
        assert!((base[2] - weight_n).abs() < 1e-7, "{base:?}");
        assert!(
            (base[4] + centroid_x_mm * weight_n).abs() < 1e-5,
            "{base:?}"
        );
        let physical = &result.member_forces[0].1;
        let base_station = physical
            .at
            .iter()
            .find(|(position, _)| *position == 0.0)
            .unwrap()
            .1;
        assert!(
            (base_station[4].hypot(base_station[5]) - physical_base_bending_arm_mm * weight_n)
                .abs()
                < 1e-5,
            "{base_station:?}"
        );
        let top_station = physical
            .at
            .iter()
            .find(|(position, _)| *position == 1.0)
            .unwrap()
            .1;
        assert!(
            top_station[4..].iter().all(|value| value.abs() < 1e-5),
            "{top_station:?}"
        );
    }
}
