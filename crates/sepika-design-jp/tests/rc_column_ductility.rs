use std::collections::HashMap;

use sepika_core::ids::ElemId;
use sepika_core::model::{MemberDetailAttr, MemberJoint, Model};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    run_member_design_checks, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
    MemberDesignCheckReport, RcColumnDuctilityApplicability as Applicability,
    RcColumnDuctilityComparison as Comparison,
};
use sepika_element::frame::beam::MemberForces;

#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod input;

const COLUMN: ElemId = ElemId(0);

fn column_model(circle: bool) -> Model {
    let mut model = input::column_model(circle, 0.0);
    let section = &mut model.sections[0];
    if let Some(SectionShape::RcColumnRect { rebar, .. }) = &section.shape {
        let shape = SectionShape::RcColumnRect {
            b: 500.0,
            d: 500.0,
            rebar: rebar.clone(),
        };
        let new = shape.to_section(section.id, section.name.clone());
        section.area = new.area;
        section.iy = new.iy;
        section.iz = new.iz;
        section.j = new.j;
        section.shape = new.shape;
        section.depth = new.depth;
        section.width = new.width;
    }
    model
}

fn run(
    model: &Model,
    rows: &[(f64, f64)],
    term: LoadTerm,
    treatment: Option<Applicability>,
) -> MemberDesignCheckReport {
    let states = treatment.map(|state| HashMap::from([(COLUMN, state)]));
    let member_forces = MemberForces {
        at: rows
            .iter()
            .map(|&(xi, compression_n)| (xi, [-compression_n, 0.0, 0.0, 0.0, 0.0, 0.0]))
            .collect(),
    };
    run_member_design_checks(
        model,
        &[(COLUMN, member_forces)],
        &[],
        &MemberDesignCheckOptions {
            term,
            rc_column_ductility_applicability: states.as_ref(),
            ..Default::default()
        },
    )
    .unwrap()
}

fn assert_comparison(actual: Comparison, expected_stress: f64, exceeds: bool) {
    assert!(
        matches!(
            actual,
            Comparison::Compared {
                compression_n_per_mm2,
                standard_n_per_mm2: 8.0,
                exceeds_standard,
            }
            if (compression_n_per_mm2 - expected_stress).abs() < 1e-12 && exceeds_standard == exceeds
        ),
        "{actual:?}"
    );
}

#[test]
fn standard_is_inclusive_for_rectangular_and_circular_gross_areas_at_all_stations() {
    for (circle, loads) in [
        (false, [1_000_000.0, 2_000_000.0, 3_000_000.0]),
        (
            true,
            [360_000.0, 720_000.0, 1_080_000.0].map(|n| n * std::f64::consts::PI),
        ),
    ] {
        let model = column_model(circle);
        let rows = [(0.0, loads[0]), (0.5, loads[1]), (1.0, loads[2])];
        let report = run(
            &model,
            &rows,
            LoadTerm::Short,
            Some(Applicability::Applicable),
        );
        let [advisory] = report.rc_column_ductility_advisories.as_slice() else {
            panic!("{report:?}");
        };
        assert_eq!(advisory.elem, COLUMN);
        assert_eq!(advisory.applicability, Applicability::Applicable);
        assert_eq!(
            advisory
                .positions
                .iter()
                .map(|(xi, _)| *xi)
                .collect::<Vec<_>>(),
            [0.0, 0.5, 1.0]
        );
        for ((_, actual), (stress, exceeds)) in
            advisory
                .positions
                .iter()
                .zip([(4.0, false), (8.0, false), (12.0, true)])
        {
            assert_comparison(*actual, stress, exceeds);
        }
        assert!(report.member_checks.iter().all(
            |(_, _, outcome)| matches!(outcome, CheckOutcome::Checked(result) if result.ok())
        ));
    }
}

#[test]
fn applicability_is_only_explicit_and_does_not_change_any_strength_outcome() {
    let model = column_model(false);
    let rows = [(0.0, 3_000_000.0), (0.5, 6_000_000.0), (1.0, f64::NAN)];
    let omitted = run(&model, &rows, LoadTerm::Short, None);
    assert!(omitted
        .member_checks
        .iter()
        .any(|(_, _, outcome)| matches!(outcome, CheckOutcome::Checked(result) if !result.ok())));
    assert!(omitted
        .member_checks
        .iter()
        .any(|(_, _, outcome)| matches!(outcome, CheckOutcome::Skipped { .. })));
    let reference = &omitted.rc_column_ductility_advisories[0];
    assert_eq!(reference.applicability, Applicability::Undetermined);
    assert_comparison(reference.positions[0].1, 12.0, true);

    for state in [
        Applicability::Applicable,
        Applicability::NotApplicable,
        Applicability::ExperimentalException,
    ] {
        let explicit = run(&model, &rows, LoadTerm::Short, Some(state));
        let advisory = &explicit.rc_column_ductility_advisories[0];
        assert_eq!(advisory.applicability, state);
        if state == Applicability::Applicable {
            assert_eq!(advisory.positions, reference.positions);
        } else {
            assert!(advisory.positions.is_empty());
        }
        assert_eq!(explicit.member_checks, omitted.member_checks);
    }
}

#[test]
fn short_compression_comparison_keeps_floating_point_roundoff_separate_from_an_excess() {
    let model = column_model(false);
    let exact_n = 2_000_000_f64;
    let rows = [
        (0.0, exact_n - 0.0001),
        (0.5, exact_n.next_up()),
        (1.0, exact_n + 0.0001),
    ];
    let report = run(
        &model,
        &rows,
        LoadTerm::Short,
        Some(Applicability::Applicable),
    );
    let positions = &report.rc_column_ductility_advisories[0].positions;
    assert!(matches!(
        positions[0].1,
        Comparison::Compared {
            exceeds_standard: false,
            ..
        }
    ));
    assert_comparison(positions[1].1, 8.0, false);
    assert!(matches!(
        positions[2].1,
        Comparison::Compared {
            exceeds_standard: true,
            ..
        }
    ));
}

#[test]
fn no_compression_missing_positions_invalid_forces_and_long_term_are_distinct() {
    let mut model = column_model(false);
    model.elements[0].rigid_zone.face_i = Some(300.0);
    model.elements[0].rigid_zone.face_j = Some(300.0);
    let mut detail = MemberDetailAttr::new(COLUMN);
    detail.joints.push(MemberJoint {
        distance: 750.0,
        kind: Default::default(),
    });
    model.member_detail_attrs.push(detail);
    let rows = [
        (0.0, 3_000_000.0),
        (0.1, 0.0),
        (0.25, -1_000_000.0),
        (0.5, f64::NEG_INFINITY),
        (1.0, 3_000_000.0),
    ];
    let short = run(
        &model,
        &rows,
        LoadTerm::Short,
        Some(Applicability::Applicable),
    );
    assert_eq!(
        short.rc_column_ductility_advisories[0].positions,
        [
            (0.1, Comparison::NoCompression),
            (0.25, Comparison::NoCompression),
            (0.5, Comparison::Unavailable),
            (0.9, Comparison::Unavailable),
        ]
    );
    let long = run(
        &model,
        &rows,
        LoadTerm::Long,
        Some(Applicability::Applicable),
    );
    assert!(long.rc_column_ductility_advisories.is_empty());
}

#[test]
fn nearby_node_center_forces_do_not_substitute_for_missing_face_forces() {
    let mut model = column_model(false);
    model.elements[0].rigid_zone.face_i = Some(0.0015);
    model.elements[0].rigid_zone.face_j = Some(0.0015);
    let xi_i = 0.0015 / 3000.0;
    let xi_j = 1.0 - 0.0015 / 3000.0;
    let node_and_mid = [(0.0, 3_000_000.0), (0.5, 0.0), (1.0, 3_000_000.0)];

    let missing = run(&model, &node_and_mid, LoadTerm::Short, None);
    assert_eq!(
        missing.rc_column_ductility_advisories[0].positions,
        [
            (xi_i, Comparison::Unavailable),
            (0.5, Comparison::NoCompression),
            (xi_j, Comparison::Unavailable),
        ]
    );

    let complete = run(
        &model,
        &[
            &node_and_mid[..],
            &[(xi_i, 1_000_000.0), (xi_j, 2_000_000.0)],
        ]
        .concat(),
        LoadTerm::Short,
        None,
    );
    let positions = &complete.rc_column_ductility_advisories[0].positions;
    assert_comparison(positions[0].1, 4.0, false);
    assert_comparison(positions[2].1, 8.0, false);
}

#[test]
fn load_state_rejection_discards_the_numeric_comparison_but_not_the_applicability() {
    let model = column_model(false);
    let rows = [(0.0, 3_000_000.0), (0.5, 3_000_000.0), (1.0, 3_000_000.0)];
    let mut report = run(&model, &rows, LoadTerm::Short, None);
    report.skip_for_load_state("同一組合せの断面力がありません");
    let advisory = &report.rc_column_ductility_advisories[0];
    assert_eq!(advisory.applicability, Applicability::Undetermined);
    assert_eq!(
        advisory.positions,
        [
            (0.0, Comparison::Unavailable),
            (0.5, Comparison::Unavailable),
            (1.0, Comparison::Unavailable)
        ]
    );
}

#[test]
fn haunch_keeps_comparison_at_the_inner_boundary_center_and_opposite_end() {
    for (i_end, near_face, within_haunch, boundary, opposite_face) in
        [(true, 0.05, 0.2, 0.3, 0.95), (false, 0.95, 0.8, 0.7, 0.05)]
    {
        let mut model = column_model(false);
        model.elements[0].rigid_zone.face_i = Some(150.0);
        model.elements[0].rigid_zone.face_j = Some(150.0);
        let mut detail = MemberDetailAttr::new(COLUMN);
        let haunch = Some(sepika_core::model::Haunch {
            length: 750.0,
            depth_increase: 100.0,
            width_increase: 0.0,
            qd1_face: None,
        });
        if i_end {
            detail.haunch_i = haunch;
        } else {
            detail.haunch_j = haunch;
        }
        detail.joints.push(MemberJoint {
            distance: within_haunch * 3000.0,
            kind: Default::default(),
        });
        model.member_detail_attrs.push(detail);
        let rows = [
            (near_face, 3_000_000.0),
            (within_haunch, 3_000_000.0),
            (boundary, 2_000_000.0),
            (0.5, 3_000_000.0),
            (opposite_face, 3_000_000.0),
        ];
        let report = run(&model, &rows, LoadTerm::Short, None);
        let positions = &report.rc_column_ductility_advisories[0].positions;
        let at = |xi| positions.iter().find(|(pos, _)| *pos == xi).unwrap().1;
        assert_eq!(at(near_face), Comparison::Unavailable);
        assert_eq!(at(within_haunch), Comparison::Unavailable);
        assert_comparison(at(boundary), 8.0, false);
        assert_comparison(at(0.5), 12.0, true);
        assert_comparison(at(opposite_face), 12.0, true);
    }
}

#[test]
fn a_clamped_station_does_not_shorten_the_haunch() {
    let mut model = column_model(false);
    let mut detail = MemberDetailAttr::new(COLUMN);
    detail.haunch_i = Some(sepika_core::model::Haunch {
        length: 1800.0,
        depth_increase: 100.0,
        width_increase: 0.0,
        qd1_face: None,
    });
    model.member_detail_attrs.push(detail);
    let rows = [(0.5, 3_000_000.0), (1.0, 3_000_000.0)];
    let report = run(&model, &rows, LoadTerm::Short, None);
    let positions = &report.rc_column_ductility_advisories[0].positions;
    assert_eq!(positions[2], (0.5, Comparison::Unavailable));
    assert_comparison(positions[3].1, 12.0, true);

    model.member_detail_attrs[0]
        .haunch_i
        .as_mut()
        .unwrap()
        .length = f64::NAN;
    let report = run(&model, &rows, LoadTerm::Short, None);
    assert!(report.rc_column_ductility_advisories[0]
        .positions
        .iter()
        .all(|(_, comparison)| *comparison == Comparison::Unavailable));
}
