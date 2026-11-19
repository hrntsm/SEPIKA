use sepika_core::model::Model;
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};
use sepika_element::frame::beam::MemberForces;

#[path = "fixtures/brb_model.rs"]
mod input;

fn check(model: &Model, axial_n: f64, term: LoadTerm) -> [CheckOutcome; 2] {
    let results = run_member_design_checks(
        model,
        &[(
            input::BRACE,
            MemberForces {
                at: vec![
                    (0.0, [axial_n, 0.0, 0.0, 0.0, 0.0, 0.0]),
                    (1.0, [axial_n, 0.0, 0.0, 0.0, 0.0, 0.0]),
                ],
            },
        )],
        &[],
        &MemberDesignCheckOptions {
            term,
            ..Default::default()
        },
    )
    .unwrap()
    .member_checks;
    assert_eq!(results.len(), 2, "{results:?}");
    let mut results = results
        .into_iter()
        .zip([0.0, 1.0])
        .map(|((id, pos, outcome), xi)| {
            assert_eq!((id, pos), (input::BRACE, xi));
            outcome
        });
    std::array::from_fn(|_| results.next().unwrap())
}

fn assert_ratio(model: &Model, axial_n: f64, term: LoadTerm, expected: f64, ok: bool) {
    for outcome in check(model, axial_n, term) {
        let CheckOutcome::Checked(result) = outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(result.components.len(), 1, "{result:?}");
        assert_eq!(result.components[0].kind, CheckKind::Axial);
        assert!((result.ratio() - expected).abs() < 1e-12, "{result:?}");
        assert_eq!(result.ok(), ok, "{result:?}");
    }
}

fn assert_skipped(model: &Model, axial_n: f64, name: &str, reason: &str) {
    for outcome in check(model, axial_n, LoadTerm::Short) {
        assert!(
            matches!(&outcome, CheckOutcome::Skipped { reason: actual }
                if actual.contains(name) && actual.contains(reason)),
            "{outcome:?}"
        );
    }
}

#[test]
fn brb_preserves_short_and_long_axial_limits() {
    let mut model = input::model();
    let attr = &mut model.brb_attrs[0];
    attr.critical_length = Some(10_000.0);
    attr.allowable_axial_short = Some(900_000.0);
    for (axial_n, term, ratio, ok) in [
        (-899_999.0, LoadTerm::Short, 0.999_998_888_889, true),
        (900_000.0, LoadTerm::Short, 1.0, true),
        (-900_001.0, LoadTerm::Short, 1.000_001_111_111, false),
        (599_999.0, LoadTerm::Long, 0.999_998_333_333, true),
        (-600_000.0, LoadTerm::Long, 1.0, true),
        (600_001.0, LoadTerm::Long, 1.000_001_666_667, false),
        (-700_000.0, LoadTerm::Long, 1.166_666_666_667, false),
    ] {
        assert_ratio(&model, axial_n, term, ratio, ok);
    }

    model.brb_attrs[0].allowable_axial_short = Some(899_999.0);
    assert_ratio(
        &model,
        599_999.333_333_333_4,
        LoadTerm::Long,
        1.0_f64.next_up(),
        false,
    );
}

#[test]
fn brb_preserves_product_length_limit_and_requires_strictly_positive_remainder() {
    let mut model = input::model();
    for (length_mm, ratio, ok) in [
        (4999.0, 0.999_772_727_273, true),
        (5000.0, 1.0, true),
        (5001.0, 1.000_227_272_727, false),
        (5440.0, 1.1, false),
    ] {
        model.nodes[1].coord[0] = length_mm;
        assert_ratio(&model, -500_000.0, LoadTerm::Short, ratio, ok);
    }

    model.nodes[1].coord[0] = 5000.0;
    model.brb_attrs[0].critical_length = Some(5000.0);
    model.brb_attrs[0].length_reduction = Some(0.0);
    assert_ratio(&model, -500_000.0, LoadTerm::Short, 1.0, true);

    model.brb_attrs[0].critical_length = Some(10_000.0);
    model.brb_attrs[0].length_reduction = Some(2500.0_f64.next_down());
    for outcome in check(&model, 0.0, LoadTerm::Short) {
        assert!(
            matches!(outcome, CheckOutcome::Checked(result) if result.ratio() > 0.0 && result.ok())
        );
    }
    for reduction_mm in [2500.0, 2500.0_f64.next_up(), f64::MAX] {
        model.brb_attrs[0].length_reduction = Some(reduction_mm);
        assert_skipped(&model, 0.0, "L−2L1", "正値");
    }
}

#[test]
fn brb_product_length_limit_is_compared_before_rounding_the_end_reduction() {
    let mut model = input::model();
    model.brb_attrs[0].critical_length = Some(5000.0_f64.next_down());
    for (single_end_reduction_mm, ratio, ok) in [
        (3.410_605_131_648_481e-13, 1.0_f64.next_up(), false),
        (4.547_473_508_864_641e-13, 1.0, true),
        (5.684_341_886_080_801e-13, 1.0, true),
    ] {
        model.brb_attrs[0].length_reduction = Some(single_end_reduction_mm);
        assert_ratio(&model, 0.0, LoadTerm::Short, ratio, ok);
    }
}

#[test]
fn brb_rejects_each_missing_and_invalid_product_value() {
    use sepika_core::model::BrbAttr;
    type ProductField = (&'static str, fn(&mut BrbAttr) -> &mut Option<f64>);
    let fields: [ProductField; 3] = [
        ("短期許容軸力", |attr| &mut attr.allowable_axial_short),
        ("限界座屈長さ", |attr| &mut attr.critical_length),
        ("端部低減距離", |attr| &mut attr.length_reduction),
    ];
    for (name, field) in fields {
        let mut model = input::model();
        *field(&mut model.brb_attrs[0]) = None;
        assert_skipped(&model, -500_000.0, name, "未入力");
        for value in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            *field(&mut model.brb_attrs[0]) = Some(value);
            assert_skipped(&model, -500_000.0, name, "有限");
        }
        if name != "端部低減距離" {
            *field(&mut model.brb_attrs[0]) = Some(0.0);
            assert_skipped(&model, -500_000.0, name, "正値");
        }
    }
}

#[test]
fn brb_reports_invalid_physical_length_with_supplied_forces() {
    for length_mm in [0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut model = input::model();
        model.nodes[1].coord[0] = length_mm;
        assert_skipped(&model, 0.0, "実材軸長", "有限の正値");
    }

    let mut model = input::model();
    model.elements[0].end_offsets_mm[1][0] = -10.0;
    assert_ratio(&model, 0.0, LoadTerm::Short, 0.997_727_272_727, true);
    model.elements[0].end_offsets_mm[1][0] = -5000.0;
    assert_skipped(&model, 0.0, "実材軸長", "解決できません");
    model.elements[0].end_offsets_mm[1][0] = f64::NAN;
    assert_skipped(&model, 0.0, "実材軸長", "XYZ偏心が非有限");

    model.elements[0].end_offsets_mm[1][0] = 0.0;
    model.elements[0].nodes.push(model.nodes[0].id);
    assert_skipped(&model, 0.0, "実材軸長", "二節点");
}

#[test]
fn brb_small_positive_limits_are_not_replaced_by_epsilon() {
    let mut model = input::model();
    model.brb_attrs[0].allowable_axial_short = Some(1e-12);
    assert_ratio(&model, 1.2e-12, LoadTerm::Short, 1.2, false);
    let smallest = f64::from_bits(1);
    model.brb_attrs[0].allowable_axial_short = Some(smallest);
    assert_ratio(&model, smallest, LoadTerm::Long, 1.5, false);
    model.brb_attrs[0].critical_length = Some(1e-10);
    model.brb_attrs[0].length_reduction = Some(2500.0 - 1.0 / 8192.0);
    assert_ratio(&model, 0.0, LoadTerm::Short, 2_441_406.25, false);
}

#[test]
fn brb_long_term_subnormal_limit_keeps_the_axial_boundary() {
    let mut model = input::model();
    model.brb_attrs[0].critical_length = Some(8800.0);
    for (limit_bits, demand_bits, ratio, ok) in [
        (6_000_000_000_000_001, 4_000_000_000_000_000, 1.0, true),
        (6_000_000_000_000_000, 4_000_000_000_000_000, 1.0, true),
        (
            6_000_000_000_000_001,
            4_000_000_000_000_001,
            1.0_f64.next_up(),
            false,
        ),
    ] {
        model.brb_attrs[0].allowable_axial_short = Some(f64::from_bits(limit_bits));
        assert_ratio(
            &model,
            -f64::from_bits(demand_bits),
            LoadTerm::Long,
            ratio,
            ok,
        );
    }
}

#[test]
fn brb_physical_length_can_be_finite_outside_the_squared_length_range() {
    let mut model = input::model();
    model.brb_attrs[0].length_reduction = Some(0.0);
    for (length_mm, limit_mm, ratio, ok) in [
        (1e155, 1e154, 10.0, false),
        (
            2.222_758_749_485_299_6e-162,
            2.222_758_749_485_188_5e-162,
            1.000_000_000_000_05,
            false,
        ),
        (3e-162, 6e-162, 0.5, true),
        (1e-200, 2e-200, 0.5, true),
    ] {
        model.nodes[1].coord[0] = length_mm;
        model.brb_attrs[0].critical_length = Some(limit_mm);
        assert_ratio(&model, 0.0, LoadTerm::Short, ratio, ok);
    }

    let smallest = f64::from_bits(1);
    model.nodes[1].coord = [smallest; 3];
    model.brb_attrs[0].critical_length = Some(smallest);
    assert_ratio(&model, 0.0, LoadTerm::Short, 2.0, false);
}

#[test]
fn brb_invalid_section_direction_retains_the_local_frame_error() {
    let mut model = input::model();
    for (end, offset, direction, reason) in [
        (
            [1e-200, 0.0, 0.0],
            [0.0; 3],
            [0.0, f64::NAN, 0.0],
            "断面参照方向に非有限値",
        ),
        (
            [5000.0, 0.0, 0.0],
            [-10.0, 0.0, 0.0],
            [0.0, f64::NAN, 0.0],
            "断面参照方向に非有限値",
        ),
        (
            [2.0, 1.0, 1.0],
            [-1.0, 0.0, 0.0],
            [1e308, 1e308, 1e308 - 1e292],
            "断面参照方向から単位直交基底を解決できません",
        ),
    ] {
        model.nodes[1].coord = end;
        model.elements[0].end_offsets_mm[1] = offset;
        model.elements[0].local_axis.ref_vector = direction;
        let error = run_member_design_checks(
            &model,
            &[(
                input::BRACE,
                MemberForces {
                    at: vec![(0.0, [0.0; 6])],
                },
            )],
            &[],
            &Default::default(),
        )
        .unwrap_err();
        assert!(error.contains(reason), "{error}");
        assert_eq!(error, model.validate_local_frames().unwrap_err());
    }
}

#[test]
fn brb_product_ratio_at_the_numeric_limit_retains_ng() {
    let mut model = input::model();
    for (limit_bits, demand_n, term) in [
        (1, -500_000.0, LoadTerm::Short),
        (7, f64::from_bits(0x3cf2_aaaa_aaaa_aaaa), LoadTerm::Long),
    ] {
        model.brb_attrs[0].allowable_axial_short = Some(f64::from_bits(limit_bits));
        for outcome in check(&model, demand_n, term) {
            let CheckOutcome::Checked(result) = outcome else {
                panic!("{outcome:?}");
            };
            assert_eq!(result.ratio(), f64::MAX);
            assert!(!result.ok());
            assert!(result.components[0].detail.contains("表現可能な最大値"));
        }
    }
}
