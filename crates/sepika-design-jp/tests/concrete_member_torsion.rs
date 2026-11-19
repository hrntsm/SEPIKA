#[path = "fixtures/cft_column_buckling_model.rs"]
mod cft;
#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod rc;

use sepika_core::ids::{ElemId, MaterialId};
use sepika_core::model::{Model, Section};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};
use sepika_element::frame::beam::MemberForces;

const COLUMN: ElemId = ElemId(0);

fn src_column() -> Model {
    let mut model = rc::column_model(false, 0.0);
    let section = &mut model.sections[0];
    let Some(SectionShape::RcColumnRect { b, d, rebar }) = section.shape.take() else {
        unreachable!()
    };
    let shape = SectionShape::SrcColumnRect {
        b,
        d,
        rebar,
        steel_height: 400.0,
        steel_width: 200.0,
        steel_web_thick: 9.0,
        steel_flange_thick: 14.0,
    };
    let mut steel = cft::cantilever(true, 0.0).materials.remove(1);
    steel.id = MaterialId(2);
    model.materials.push(steel);
    *section = Section {
        frame_use: section.frame_use,
        material: section.material,
        rebar_material: section.rebar_material,
        shear_rebar_material: section.shear_rebar_material,
        steel_material: Some(MaterialId(2)),
        ..shape.to_section(section.id, "SRC柱".into())
    };
    model.validate().unwrap();
    model
}

fn run(model: &Model, forces: Vec<(f64, [f64; 6])>) -> Vec<(ElemId, f64, CheckOutcome)> {
    run_member_design_checks(
        model,
        &[(COLUMN, MemberForces { at: forces })],
        &[],
        &MemberDesignCheckOptions {
            term: LoadTerm::Short,
            ..Default::default()
        },
    )
    .unwrap()
    .member_checks
}

fn complete_cft() -> Model {
    let mut model = cft::cantilever(true, 0.0);
    model.steel_design_attrs = vec![cft::direct_lengths(COLUMN, Some(6000.0), Some(6000.0))];
    model
}

#[test]
fn torsion_is_reported_only_at_the_affected_design_positions() {
    for model in [rc::column_model(false, 0.0), src_column(), complete_cft()] {
        let negative_zero = (0.0, [0.0, 0.0, 0.0, -0.0, 0.0, 0.0]);
        let pure_mx_nmm = 100.0e6;
        let pure = (0.5, [0.0, 0.0, 0.0, pure_mx_nmm, 0.0, 0.0]);
        let mut combined = (1.0, [-100_000.0, 1000.0, 1000.0, -1.0e6, 1.0e6, 1.0e6]);
        let rows = run(&model, vec![negative_zero, pure, combined]);
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert_eq!((rows[0].0, rows[0].1), (COLUMN, 0.0));
        assert!(
            matches!(&rows[0].2, CheckOutcome::Checked(result) if result.ok()),
            "{rows:?}"
        );
        assert_eq!((rows[1].0, rows[1].1), (COLUMN, 0.5));
        assert!(
            matches!(&rows[1].2, CheckOutcome::Skipped { reason } if reason.contains("純ねじり（Mx）")),
            "{rows:?}"
        );
        assert_eq!((rows[2].0, rows[2].1), (COLUMN, 1.0));
        assert!(
            matches!(&rows[2].2, CheckOutcome::Skipped { reason } if reason.contains("Mx") && reason.contains("同時に作用")),
            "{rows:?}"
        );

        let smallest_mx_nmm = f64::from_bits(1);
        combined.1[3] = smallest_mx_nmm;
        let rows = run(&model, vec![combined]);
        assert!(
            matches!(&rows[..], [(_, 1.0, CheckOutcome::Skipped { reason })]
                if reason.contains("Mx") && reason.contains("同時に作用")),
            "{rows:?}"
        );

        combined.1[3] = 0.0;
        let rows = run(&model, vec![combined]);
        assert!(
            matches!(&rows[..], [(_, 1.0, CheckOutcome::Checked(result))] if result.ok()),
            "{rows:?}"
        );
    }
}

#[test]
fn torsion_diagnostics_and_independent_ng_coexist_at_the_same_position() {
    for model in [rc::column_model(false, 0.0), src_column(), complete_cft()] {
        let without_torsion = [-30.0e6, 0.0, 0.0, 0.0, 0.0, 0.0];
        let baseline = run(&model, vec![(0.5, without_torsion)]);
        let [(_, _, CheckOutcome::Checked(baseline))] = &baseline[..] else {
            panic!("{baseline:?}");
        };
        assert!(!baseline.ok());
        for (mx, expected) in [
            (1.0e6, "同時に作用"),
            (f64::NAN, "非有限"),
            (f64::INFINITY, "非有限"),
            (f64::NEG_INFINITY, "非有限"),
        ] {
            let mut forces = without_torsion;
            forces[3] = mx;
            let rows = run(&model, vec![(0.5, forces)]);
            let [(_, 0.5, CheckOutcome::Checked(result)), (_, 0.5, CheckOutcome::Skipped { reason })] =
                &rows[..]
            else {
                panic!("{rows:?}");
            };
            assert_eq!(result.ratio(), baseline.ratio());
            assert!(result
                .components
                .iter()
                .all(|component| component.ratio > 1.0));
            assert!(
                reason.contains("Mx") && reason.contains(expected),
                "{reason}"
            );
        }
    }
}

#[test]
fn rc_provision_ng_remains_even_under_pure_torsion() {
    let mut model = rc::column_model(false, 0.0);
    let Some(SectionShape::RcColumnRect { rebar, .. }) = &mut model.sections[0].shape else {
        unreachable!()
    };
    rebar.hoop.pitch = 125.0;
    let rows = run(&model, vec![(0.5, [0.0, 0.0, 0.0, 100.0e6, 0.0, 0.0])]);
    assert!(
        matches!(&rows[..],
            [(_, 0.5, CheckOutcome::Checked(result)), (_, 0.5, CheckOutcome::Skipped { reason })]
            if !result.ok()
                && result.components.len() == 1
                && result.components[0].kind == CheckKind::Provision
                && result.components[0].detail.contains("帯筋間隔")
                && reason.contains("純ねじり（Mx）")),
        "{rows:?}"
    );
}

#[test]
fn existing_missing_cft_buckling_length_reason_remains_with_torsion() {
    let model = cft::cantilever(true, 0.0);
    let rows = run(&model, vec![(0.0, [0.0, 0.0, 0.0, 1000.0, 0.0, 0.0])]);
    assert!(
        matches!(&rows[..], [(_, 0.0, CheckOutcome::Skipped { reason })]
            if reason.contains("座屈長さ") && reason.contains("純ねじり（Mx）")),
        "{rows:?}"
    );
}
