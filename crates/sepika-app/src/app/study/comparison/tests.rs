use super::super::tests::{app, assign, wait};
use super::{projection::*, *};
use references::{EvidenceReference, ReferenceTarget};

pub(in crate::app::study) fn computed() -> App {
    let mut app = app();
    app.core
        .model
        .combinations
        .push(sepika_core::model::LoadCombination {
            name: "長期＋地震X".into(),
            terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)],
        });
    app.create_study("比較回帰".into(), vec![ElemId(2)])
        .unwrap();
    app.create_comparison(0, "案A".into()).unwrap();
    app.create_comparison(0, "案B".into()).unwrap();
    app.switch_study(Location::Proposal(0, 1)).unwrap();
    assert!(assign(&mut app, 2));
    for p in 0..3 {
        app.start_study_calculation(0, p).unwrap();
        wait(&mut app);
    }
    app
}

#[test]
fn fixed_volume_signed_displacement_and_every_case_design_match_normal_results_and_roundtrip() {
    let mut app = computed();
    let fixed = fixed_members(&app.studies.items[0]);
    assert_eq!(fixed, [ElemId(2)]);
    let before = bincode::serialize(&app.saved_studies()).unwrap();
    for run in &app.studies.items[0].runs {
        let source = result_app(
            &run.prepared,
            run.results.as_ref().unwrap(),
            run.last_static,
        );
        let quantity = source.member_quantity_evidence_reference(fixed.clone());
        let takeoff = quantity.takeoff().unwrap();
        assert_eq!(takeoff.items.len(), 1);
        let section = run.input.model.element(ElemId(2)).unwrap().section.unwrap();
        let expected_m3 = run.input.model.section(section).unwrap().area * 6000.0 / 1e9;
        assert!(
            (takeoff.totals().steel_t / sepika_core::units::STEEL_UNIT_WEIGHT_TAKEOFF_T_M3
                - expected_m3)
                .abs()
                < 1e-12
        );
        assert!(same_conditions(
            &app.studies.items[0].baseline,
            &run.input,
            &fixed
        ));
        let bundle = run.results.as_ref().unwrap();
        assert_eq!(
            run.comparison_checks.len(),
            bundle.statics.len() + bundle.combos.len()
        );
        let original_one_case_rows = bundle
            .member_checks
            .iter()
            .map(|m| m.positions.len())
            .sum::<usize>();
        let mut all = Vec::new();
        for evidence in &run.comparison_checks {
            let rows = check_rows(evidence).unwrap();
            let mut normal = result_app(&evidence.input, &evidence.bundle, evidence.last_static);
            normal.run_design_check();
            let normal_bundle = normal.core.scoped.results.as_ref().unwrap();
            assert_eq!(
                bincode::serialize(&normal_bundle.member_checks).unwrap(),
                bincode::serialize(&evidence.bundle.member_checks).unwrap()
            );
            for row in &rows {
                if let Some(reference) = &row.evidence {
                    assert_eq!(
                        app.study_result_location(reference),
                        Some(Location::History(
                            0,
                            app.studies.items[0]
                                .runs
                                .iter()
                                .position(|r| r.proposal == run.proposal)
                                .unwrap()
                        ))
                    );
                    assert!(app
                        .study_result_view(reference)
                        .unwrap()
                        .resolve_result_reference(reference)
                        .is_ok());
                }
            }
            all.extend(rows);
        }
        assert!(all.len() > original_one_case_rows);
        assert!(!bundle.combos.is_empty());
        for (i, (name, result)) in bundle.combos.iter().enumerate() {
            let (value, reference) =
                displacement(&source, StaticKey::Combo(i), NodeId(3), 0).unwrap();
            assert_eq!(value, result.disp[3][0]);
            let EvidenceReference::Calculation(reference) = reference else {
                panic!()
            };
            assert_eq!(reference.kind, ResultInputKey::Combo(name.clone()));
        }
        let key = StaticKey::Case(StaticCaseKey::User(LoadCaseId(0)));
        let (negative, reference) = displacement(&source, key, NodeId(3), 2).unwrap();
        assert!(negative < 0.0);
        assert_eq!(
            negative,
            bundle
                .statics
                .iter()
                .find(|(k, _)| *k == StaticCaseKey::User(LoadCaseId(0)))
                .unwrap()
                .1
                .disp[3][2]
        );
        let EvidenceReference::Calculation(reference) = reference else {
            panic!()
        };
        assert_eq!(reference.display.direction.as_deref(), Some("Z"));
        assert_eq!(reference.display.unit.as_deref(), Some("mm"));
        let (zero, _) = displacement(&source, key, NodeId(0), 2).unwrap();
        assert_eq!(zero, 0.0);
        assert!(displacement(&source, key, NodeId(999), 0).is_err());
    }
    assert_eq!(before, bincode::serialize(&app.saved_studies()).unwrap());
    let run = &app.studies.items[0].runs[0];
    let quantity = result_app(
        &run.prepared,
        run.results.as_ref().unwrap(),
        run.last_static,
    )
    .member_quantity_evidence_reference(fixed.clone());
    app.save_study_quantity_reference(quantity.clone()).unwrap();
    let before = bincode::serialize(&app.saved_studies()).unwrap();
    let saved: SavedStudies =
        rmp_serde::from_slice(&rmp_serde::to_vec_named(&app.saved_studies()).unwrap()).unwrap();
    let mut loaded = App::default();
    loaded.core.scoped.references.project = app.core.scoped.references.project.clone();
    loaded.restore_studies(saved).unwrap();
    assert_eq!(before, bincode::serialize(&loaded.saved_studies()).unwrap());
    assert_eq!(
        loaded.studies.items[0].quantity_evidence[0].identity,
        quantity.identity
    );
    assert!(
        matches!(&loaded.studies.items[0].quantity_evidence[0].scope, references::QuantityEvidenceScope::Members(ids) if ids == &fixed)
    );
    for run in &loaded.studies.items[0].runs {
        for evidence in &run.comparison_checks {
            assert!(check_rows(evidence).is_ok());
        }
    }
    let directory =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/issue980-contract");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("roundtrip-{}.ovika", uuid::Uuid::new_v4()));
    app.save_project_to(path.clone());
    assert!(app.core.scoped.last_error.is_none());
    let mut from_file = App::default();
    from_file.open_project_from(path.clone());
    assert!(from_file.core.scoped.last_error.is_none());
    assert_eq!(
        before,
        bincode::serialize(&from_file.saved_studies()).unwrap()
    );
    std::fs::remove_file(path).unwrap();
    let baseline_rows: Vec<_> = from_file.studies.items[0].runs[0]
        .comparison_checks
        .iter()
        .flat_map(|e| check_rows(e).unwrap())
        .collect();
    let proposal_rows: Vec<_> = from_file.studies.items[0].runs[1]
        .comparison_checks
        .iter()
        .flat_map(|e| check_rows(e).unwrap())
        .collect();
    let outside = member_impacts(&baseline_rows, &proposal_rows, &fixed);
    assert!(outside.iter().any(|impact| impact.after.target
        == Some(ReferenceTarget::Member(ElemId(0)))
        && impact.difference.is_some_and(|d| d > 0.0)));
}

#[test]
fn conditions_compare_actual_definitions_not_names_ids_or_governing_case() {
    let app = app();
    let original = StudyInput::capture(&app.core);
    let mut changed = original.clone();
    changed.model.elements[2].section = Some(SectionId(2));
    assert!(same_conditions(&original, &changed, &[ElemId(2)]));
    changed.model.load_cases[1].nodal[0].values[0] *= 2.0;
    assert!(!same_conditions(&original, &changed, &[ElemId(2)]));
    assert!(condition_differences(&original, &changed, &[ElemId(2)])
        .iter()
        .any(|s| s.contains("ユーザー入力荷重")));
    let mut changed = original.clone();
    changed.model.nodes[0].restraint = sepika_core::dof::Dof6Mask::FREE;
    assert!(condition_differences(&original, &changed, &[ElemId(2)])
        .iter()
        .any(|s| s.contains("支点")));
    let mut changed = original.clone();
    changed.settings.analysis_cfg.threads += 1;
    assert!(!same_conditions(&original, &changed, &[ElemId(2)]));
    let mut changed = original.clone();
    changed
        .model
        .combinations
        .push(sepika_core::model::LoadCombination {
            name: "同名".into(),
            terms: vec![(LoadCaseId(0), 1.0)],
        });
    assert!(!same_conditions(&original, &changed, &[ElemId(2)]));
    let mut baseline = original.clone();
    baseline.model.combinations = vec![sepika_core::model::LoadCombination {
        name: "同名".into(),
        terms: vec![(LoadCaseId(0), 1.0)],
    }];
    let mut changed = baseline.clone();
    changed.model.combinations[0].terms[0].1 = 2.0;
    assert!(!same_conditions(&baseline, &changed, &[ElemId(2)]));
}

fn row(case: &str, elem: u32, value: Option<f64>) -> CheckRow {
    CheckRow {
        key: format!("{case}/部材{elem}/位置0"),
        case: case.into(),
        target: Some(ReferenceTarget::Member(ElemId(elem))),
        position: Some(0.0),
        ratio: value,
        complete: value.is_some(),
        invalid_components: false,
        reason: if value.is_none() {
            "未対応".into()
        } else {
            "算定済".into()
        },
        items: vec![sepika_design_jp::CheckKind::AxialBending],
        governing_items: vec![sepika_design_jp::CheckKind::AxialBending],
        evidence: None,
    }
}

#[test]
fn envelope_allows_governing_case_change_and_counts_skips_and_outside_new_ng_at_same_granularity() {
    let baseline = vec![
        row("A", 0, Some(0.8)),
        row("B", 0, Some(0.7)),
        row("A", 2, Some(0.9)),
        row("B", 2, None),
    ];
    let proposal = vec![
        row("A", 0, Some(0.6)),
        row("B", 0, Some(1.2)),
        row("A", 2, Some(0.4)),
        row("B", 2, None),
    ];
    assert!(same_evaluation_set(&baseline, &proposal));
    let before = summarize(&baseline);
    let after = summarize(&proposal);
    assert_eq!(before.maximum.unwrap().case, "A");
    assert_eq!(after.maximum.unwrap().case, "B");
    assert_eq!((after.checked, after.ng, after.skipped), (3, 1, 1));
    let worsening = worsening(&baseline, &proposal, &[ElemId(2)]);
    assert_eq!(worsening.len(), 1);
    assert!(worsening[0].2);
    assert_eq!(
        worsening[0].0.target,
        Some(ReferenceTarget::Member(ElemId(0)))
    );
    assert_eq!(summarize(&[]).maximum, None);
}

#[test]
fn invalid_design_components_and_stale_or_missing_saved_inputs_never_become_valid_zero() {
    let mut app = computed();
    let run = &mut app.studies.items[0].runs[0];
    let mut evidence = run.comparison_checks[0].clone();
    let check = &mut evidence.bundle.member_checks[0].positions[0].outcome;
    if let sepika_design_jp::CheckOutcome::Checked(check) = check {
        check.components.clear();
    }
    assert!(check_rows(&evidence).is_err());
    let mut evidence = run.comparison_checks[0].clone();
    if let sepika_design_jp::CheckOutcome::Checked(check) =
        &mut evidence.bundle.member_checks[0].positions[0].outcome
    {
        check.components[0].ratio = f64::NAN;
    }
    assert!(check_rows(&evidence).unwrap()[0].ratio.is_none());
    let mut old = run.comparison_checks[0].clone();
    old.bundle.design_contract_version = Some(0);
    assert!(check_rows(&old).is_err());
    let mut missing = run.comparison_checks[0].clone();
    missing.bundle.input_records.clear();
    assert!(check_rows(&missing).is_err());
    let source = result_app(
        &run.prepared,
        run.results.as_ref().unwrap(),
        run.last_static,
    );
    assert!(source
        .member_quantity_evidence_reference(vec![])
        .takeoff()
        .is_err());
    assert!(source
        .member_quantity_evidence_reference(vec![ElemId(999)])
        .takeoff()
        .is_err());
    let quantity = source.member_quantity_evidence_reference(vec![ElemId(2)]);
    let mut tampered = quantity.clone();
    tampered.scope = references::QuantityEvidenceScope::WholeModel;
    assert_ne!(quantity.identity(), tampered.identity());
}

#[test]
fn moved_or_reordered_positions_are_not_paired_by_index_and_member_case_maxima_keep_actual_positions(
) {
    fn located(xi: f64, value: Option<f64>) -> CheckRow {
        let mut r = row("A", 0, value);
        r.position = Some(xi);
        r.key = format!("A/部材0/位置{:016x}", xi.to_bits());
        r
    }
    let baseline = vec![located(0.1, Some(0.8)), located(0.9, Some(0.7))];
    let reordered = vec![located(0.9, Some(1.2)), located(0.1, Some(0.6))];
    let differences = worsening(&baseline, &reordered, &[]);
    assert_eq!(differences.len(), 1);
    assert_eq!(differences[0].0.position, Some(0.9));
    assert!((differences[0].1 - 0.5).abs() < 1e-12);
    let maxima = member_impacts(&baseline, &reordered, &[]);
    assert!(maxima[0].new_ng);
    assert!(!maxima[0].positions_changed);
    let moved = vec![located(0.05, Some(0.6)), located(0.95, Some(1.2))];
    assert!(worsening(&baseline, &moved, &[]).is_empty());
    assert_eq!(unmatched_positions(&baseline, &moved), (2, 2));
    assert!(same_check_scope(&baseline, &moved));
    let maxima = member_impacts(&baseline, &moved, &[]);
    assert_eq!(maxima.len(), 1);
    assert_eq!(maxima[0].before.as_ref().unwrap().position, Some(0.1));
    assert_eq!(maxima[0].after.position, Some(0.95));
    assert!((maxima[0].difference.unwrap() - 0.4).abs() < 1e-12);
    assert!(maxima[0].positions_changed);
    assert!(!maxima[0].new_ng);
    assert_eq!((maxima[0].skipped_before, maxima[0].skipped_after), (0, 0));
    let skipped = vec![located(0.05, None), located(0.95, Some(0.8))];
    let maxima = member_impacts(&skipped, &moved, &[]);
    assert_eq!(maxima[0].skipped_before, 1);
    assert!(!maxima[0].new_ng);
}

#[test]
fn fixed_member_maximum_is_independent_of_dominating_outside_column() {
    let rows = vec![
        row("A", 0, Some(1.8)),
        row("B", 2, Some(0.7)),
        row("A", 2, Some(0.5)),
        row("B", 1, None),
    ];
    let subset = fixed_rows(&rows, &[ElemId(2)]);
    let fixed = summarize(subset.iter().copied());
    let all = summarize(&rows);
    assert_eq!(
        fixed.maximum.unwrap().target,
        Some(ReferenceTarget::Member(ElemId(2)))
    );
    assert_eq!(fixed.checked, 2);
    assert_eq!(
        all.maximum.unwrap().target,
        Some(ReferenceTarget::Member(ElemId(0)))
    );
    assert_eq!((all.ng, all.skipped), (1, 1));
    assert_eq!((fixed.ng, fixed.skipped), (0, 0));
}

#[test]
fn member_case_envelope_does_not_subtract_different_check_items_or_incomplete_ranges() {
    let baseline = vec![row("A", 0, Some(0.8))];
    let mut changed = vec![row("A", 0, Some(1.2))];
    changed[0].items = vec![sepika_design_jp::CheckKind::Provision];
    changed[0].governing_items = changed[0].items.clone();
    let impact = member_impacts(&baseline, &changed, &[]).remove(0);
    assert!(impact.items_changed);
    assert!(!impact.positions_changed);
    assert_eq!(impact.difference, None);
    assert!(!impact.new_ng);
    let skipped = vec![row("A", 0, None)];
    let impact = member_impacts(&skipped, &baseline, &[]).remove(0);
    assert_eq!(impact.difference, None);
    assert!(!impact.new_ng);
    assert_eq!(impact.skipped_before, 1);
}

#[test]
fn equal_skipped_counts_do_not_hide_different_rated_positions_but_fully_rated_maxima_allow_position_moves(
) {
    fn located(xi: f64, ratio: Option<f64>) -> CheckRow {
        let mut r = row("A", 2, ratio);
        r.position = Some(xi);
        r.key = format!("A/部材2/位置{:016x}", xi.to_bits());
        r
    }
    let baseline = vec![located(0.1, None), located(0.9, Some(0.8))];
    let swapped = vec![located(0.1, Some(1.2)), located(0.9, None)];
    assert_eq!(summarize(&baseline).skipped, summarize(&swapped).skipped);
    assert!(same_check_scope(&baseline, &swapped));
    assert!(!same_evaluation_range(&baseline, &swapped));
    assert!(!same_evaluation_range(
        fixed_rows(&baseline, &[ElemId(2)]),
        fixed_rows(&swapped, &[ElemId(2)])
    ));
    assert!(same_evaluation_range(&baseline, &baseline));
    let rated = vec![located(0.1, Some(0.8)), located(0.9, Some(0.7))];
    let moved = vec![located(0.05, Some(0.6)), located(0.95, Some(1.2))];
    assert!(same_evaluation_range(&rated, &moved));
}

#[test]
fn saved_expected_targets_positions_and_component_multisets_reject_missing_result_structure() {
    let app = computed();
    let original = app.studies.items[0].runs[0].comparison_checks[0].clone();
    let expected = rmp_serde::to_vec_named(&original.expected_checks).unwrap();
    for change in 0..7 {
        let mut evidence = original.clone();
        match change {
            0 => evidence
                .bundle
                .member_checks
                .retain(|m| m.elem != ElemId(2)),
            1 => evidence
                .bundle
                .member_checks
                .retain(|m| m.elem != ElemId(0)),
            2 => evidence
                .bundle
                .member_checks
                .iter_mut()
                .find(|m| m.elem == ElemId(2))
                .unwrap()
                .positions
                .clear(),
            3 => {
                evidence.bundle.member_checks[0].positions.pop();
            }
            4 => {
                let check = evidence
                    .bundle
                    .member_checks
                    .iter_mut()
                    .flat_map(|m| &mut m.positions)
                    .find_map(|p| match &mut p.outcome {
                        sepika_design_jp::CheckOutcome::Checked(c)
                            if c.components.len() > 1
                                && c.ratio() > 0.0
                                && c.components.iter().filter(|v| v.ratio == c.ratio()).count()
                                    == 1 =>
                        {
                            Some(c)
                        }
                        _ => None,
                    })
                    .expect("複数項目を実計算するfixture");
                let before = check.ratio();
                let index = check
                    .components
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.ratio.total_cmp(&b.1.ratio))
                    .unwrap()
                    .0;
                check.components.remove(index);
                assert!(result_validity::checked_ratio(check).is_some());
                assert!(check.ratio() < before);
            }
            5 => {
                evidence.bundle.joint_checks.pop();
            }
            _ => evidence.expected_checks = None,
        }
        let before = rmp_serde::to_vec_named(&evidence).unwrap();
        let restored: ReferencedResult = rmp_serde::from_slice(&before).unwrap();
        assert!(check_rows(&restored).is_err(), "改変 {change}");
        if change != 6 {
            assert_eq!(
                expected,
                rmp_serde::to_vec_named(&restored.expected_checks).unwrap()
            );
        }
        assert_eq!(before, rmp_serde::to_vec_named(&restored).unwrap());
        if matches!(change, 0 | 2) {
            assert!(fixed_coverage(&restored, &[ElemId(2)]).is_err());
        }
    }
}

#[test]
fn same_position_partial_ng_is_both_ng_and_unrated_without_comparable_difference() {
    use sepika_design_jp::{CheckComponent, CheckKind, CheckOutcome, CheckResult};
    let app = computed();
    for invalid in [f64::NAN, f64::INFINITY, -0.5] {
        for valid in [Some(1.2), Some(0.6), None] {
            let mut evidence = app.studies.items[0].runs[0].comparison_checks[0].clone();
            let mut components = vec![CheckComponent {
                kind: CheckKind::Shear,
                ratio: invalid,
                detail: String::new(),
            }];
            if let Some(ratio) = valid {
                components.push(CheckComponent {
                    kind: CheckKind::Bending,
                    ratio,
                    detail: String::new(),
                });
            }
            evidence.bundle.member_checks[0].positions[0].outcome =
                CheckOutcome::Checked(CheckResult {
                    basis: String::new(),
                    detail: String::new(),
                    components,
                });
            evidence.expected_checks = Some(ExpectedChecks::capture(&evidence.bundle));
            let before = rmp_serde::to_vec_named(&evidence).unwrap();
            let rows = check_rows(&evidence).unwrap();
            let row = &rows[0];
            let summary = summarize([row]);
            assert!(!row.complete);
            assert_eq!(
                (summary.checked, summary.skipped, summary.ng),
                (0, 1, usize::from(valid == Some(1.2)))
            );
            assert_eq!(row.ratio, valid.filter(|r| *r > 1.0));
            assert!(!same_evaluation_range([row], [row]));
            assert!(
                worsening(std::slice::from_ref(row), std::slice::from_ref(row), &[]).is_empty()
            );
            assert_eq!(before, rmp_serde::to_vec_named(&evidence).unwrap());
        }
    }
}
