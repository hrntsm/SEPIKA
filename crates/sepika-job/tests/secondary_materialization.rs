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
