use sepika_core::ids::ElemId;
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};
use sepika_element::frame::beam::MemberForces;

#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod input;

#[test]
fn observe_short_axial_warning_through_product_entry() {
    for circle in [false, true] {
        let mut model = input::column_model(circle, 0.0);
        if !circle {
            let section = &mut model.sections[0];
            let mut shape = section.shape.clone().unwrap();
            let SectionShape::RcColumnRect { b, d, .. } = &mut shape else {
                unreachable!()
            };
            *b = 500.0;
            *d = 500.0;
            let mut square = shape.to_section(section.id, section.name.clone());
            square.material = section.material;
            square.rebar_material = section.rebar_material;
            square.shear_rebar_material = section.shear_rebar_material;
            square.frame_use = section.frame_use;
            *section = square;
        }
        let gross_area_mm2 = if circle {
            90_000.0 * std::f64::consts::PI
        } else {
            250_000.0
        };
        for term in [LoadTerm::Long, LoadTerm::Short] {
            for (sigma_n_mm2, short_warning) in [
                (-12.0, false),
                (-8.0, false),
                (-4.0, false),
                (0.0, false),
                (4.0, true),
                (8.0 - 2e-9, true),
                (8.0 - 0.5e-9, false),
                (8.0, false),
                (8.0 + 2e-9, false),
                (12.0, false),
            ] {
                let compression_n = sigma_n_mm2 * gross_area_mm2;
                let forces = vec![(
                    ElemId(0),
                    MemberForces {
                        at: [0.0, 0.5, 1.0]
                            .into_iter()
                            .map(|pos| (pos, [-compression_n, 0.0, 0.0, 0.0, 0.0, 0.0]))
                            .collect(),
                    },
                )];
                let report = run_member_design_checks(
                    &model,
                    &forces,
                    &[],
                    &MemberDesignCheckOptions {
                        term,
                        ..Default::default()
                    },
                )
                .unwrap();
                assert_eq!(report.member_checks.len(), 3);
                let mut ratio = None;
                for (_, pos, outcome) in report.member_checks {
                    let CheckOutcome::Checked(result) = outcome else {
                        panic!("circle={circle}, term={term:?}, sigma={sigma_n_mm2}: {outcome:?}");
                    };
                    let warning = result.detail.contains("短期軸応力度");
                    assert_eq!(
                        warning,
                        term == LoadTerm::Short && pos == 0.5 && short_warning,
                        "circle={circle}, term={term:?}, sigma={sigma_n_mm2}, pos={pos}: {}",
                        result.detail
                    );
                    assert!(result
                        .components
                        .iter()
                        .all(|c| c.kind != CheckKind::Provision));
                    if let Some(previous) = ratio {
                        assert_eq!(result.ratio(), previous);
                    }
                    ratio = Some(result.ratio());
                    println!("circle={circle}, term={term:?}, sigma={sigma_n_mm2:.12}, N={compression_n:.12}, pos={pos}, warning={warning}, ratio={}, ok={}", result.ratio(), result.ok());
                }
            }
        }
    }
}
