use sepika_core::model::Model;
use sepika_design_jp::member_design_check::{run_member_design_checks, MemberDesignCheckOptions};
use sepika_design_jp::{CheckKind, CheckOutcome, LoadTerm};

#[path = "fixtures/steel_column_lateral_buckling_model.rs"]
mod input;

fn checks(model: &Model, mz_nmm: f64) -> Result<Vec<CheckOutcome>, String> {
    run_member_design_checks(
        model,
        &[(model.elements[0].id, input::moments(mz_nmm))],
        &[],
        &MemberDesignCheckOptions {
            term: LoadTerm::Short,
            ..Default::default()
        },
    )
    .map(|report| {
        report
            .member_checks
            .into_iter()
            .map(|(_, _, outcome)| outcome)
            .collect()
    })
}

#[test]
fn confirmed_end_supports_use_the_eccentric_physical_length() {
    for (offset_mm, lb_mm, expected, ok) in [
        (0.0, 3000, 0.973_509_006_066, true),
        (500.0, 3500, 1.039_363_326_692, false),
    ] {
        let mut model = input::column(offset_mm, true);
        model.steel_design_attrs[0].lb_direct = Some((450.0, 350.0, 250.0));
        let outcomes = checks(&model, 215e6).unwrap();
        assert_eq!(outcomes.len(), 3);
        for outcome in outcomes {
            let CheckOutcome::Checked(result) = outcome else {
                panic!("{outcome:?}");
            };
            assert!(result.detail.contains(&format!("横座屈長さ lb={lb_mm} mm")));
            assert!(result.detail.contains("中間横補剛なし"));
            assert!(result.detail.contains("梁用のlb直接入力は不使用"));
            let axial_bending = result
                .components
                .iter()
                .find(|component| component.kind == CheckKind::AxialBending)
                .unwrap();
            assert!((axial_bending.ratio - expected).abs() < 1e-11, "{result:?}");
            assert_eq!(result.ok(), ok, "{result:?}");
        }
    }
}

#[test]
fn unconfirmed_or_intermediate_supports_do_not_provide_a_column_buckling_length() {
    for (confirmed, count, direct, reason) in [
        (false, None, None, "両実材端での横移動・ねじれの拘束"),
        (false, Some(0), None, "中間横補剛なしが未確認"),
        (
            false,
            None,
            Some((1000.0, 1500.0, 1000.0)),
            "実材端以外の支点",
        ),
        (true, Some(1), None, "中間横補剛が入力されています"),
    ] {
        let mut model = input::column(500.0, confirmed);
        model.steel_design_attrs[0].lateral_brace_count = count;
        model.steel_design_attrs[0].lb_direct = direct;
        let outcomes = checks(&model, 215e6).unwrap();
        assert_eq!(outcomes.len(), 3);
        for outcome in outcomes {
            assert!(
                matches!(&outcome, CheckOutcome::Skipped { reason: actual } if actual.contains(reason)),
                "{outcome:?}"
            );
        }
    }
}

#[test]
fn failure_lower_bound_and_missing_support_are_both_reported() {
    let model = input::column(500.0, false);
    let outcomes = checks(&model, 300e6).unwrap();
    assert_eq!(outcomes.len(), 6);
    for pair in outcomes.chunks_exact(2) {
        let [CheckOutcome::Checked(result), CheckOutcome::Skipped { reason }] = pair else {
            panic!("{pair:?}");
        };
        assert!(!result.ok());
        let check = &result.components[0];
        assert_eq!(check.kind, CheckKind::AxialBending);
        assert!(check.detail.contains("軸曲げ比の下限"));
        assert!(reason.contains("中間横補剛なしが未確認"));
    }
    let outcomes = checks(&model, 0.0).unwrap();
    assert_eq!(outcomes.len(), 3);
    assert!(outcomes
        .iter()
        .all(|outcome| matches!(outcome, CheckOutcome::Checked(result) if result.ok())));
}

#[test]
fn nonfinite_end_offset_is_rejected_even_with_confirmed_supports() {
    let mut model = input::column(500.0, true);
    model.elements[0].end_offsets_mm[1][2] = f64::NAN;
    let reason = checks(&model, 215e6).unwrap_err();
    assert!(reason.contains("j端のXYZ偏心が非有限"), "{reason}");
}
