#[path = "../../sepika-core/tests/support/secondary_materialization_model.rs"]
mod fixture;
use sepika_core::{ids::*, model::*};

#[test]
fn independent_beam_keeps_self_weight_transfer_and_design_target() {
    for (primary, secondary, expected_count) in [
        (Some(-300.), Some(0.), 1),
        (None, Some(0.), 1),
        (Some(-300.), Some(-300.), 0),
        (None, None, 0),
    ] {
        let mut model = fixture::model();
        model.elements[0].vertical_eccentricity = primary;
        model.unassigned_beams[0].vertical_eccentricity = secondary;
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
    model.elements[0].vertical_eccentricity = Some(0.);
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
