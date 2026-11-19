#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod input;

use sepika_core::{
    dof::Dof6Mask,
    ids::ElemId,
    model::MaterialCategory,
    rc_beam_backbone::rc_beam_alpha_evaluation,
    rc_capacity::RcAlphaSection,
    section_shape::{one_bar_area, SectionShape},
};
use sepika_design_jp::{
    ultimate::{collect_rc_ultimate_checks, MemberDemand, UltimateShearOptions},
    BondMethod, CheckKind, CheckOutcome, DesignCheck, DesignCtx, MemberForcesAt, RcDesign,
};
use std::f64::consts::PI;

#[test]
fn public_entry_observations() {
    let mut model = input::model(false);
    model.nodes[1].coord[0] = 4000.0;
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.ultimate_gravity_combination = Some(0);
    model.elements[0].rigid_zone.face_i = Some(0.0);
    model.elements[0].rigid_zone.face_j = Some(0.0);
    model.sections[0] = input::section(350.0, 609.5, 19.0, &[2], &[4]);
    let opts = UltimateShearOptions::default();
    for (case, mz, q_long) in [
        ("asymmetric_top", -10e6, 0.0),
        ("asymmetric_bottom", 10e6, 0.0),
    ] {
        let demand = MemberDemand {
            mz,
            q_long: Some(q_long),
            ..Default::default()
        };
        let check = &collect_rc_ultimate_checks(&model, &[(ElemId(0), demand)], &opts).unwrap()[0];
        println!("{case}: {check:?}");
    }

    model.sections[0] = input::section(350.0, 609.5, 19.0, &[2], &[2]);
    let opts = UltimateShearOptions {
        include_bond: false,
        ..Default::default()
    };
    for (case, demand) in [
        (
            "symmetric_static",
            MemberDemand {
                mz: -10e6,
                q_long: Some(560_400.0),
                ..Default::default()
            },
        ),
        (
            "confirmed_total",
            MemberDemand::from_pushover(0.0, -10e6, 0.0, 580_000.0, 0.0, 0.0),
        ),
    ] {
        let check = &collect_rc_ultimate_checks(&model, &[(ElemId(0), demand)], &opts).unwrap()[0];
        println!("{case}: {check:?}");
    }

    for dia in [19.0, 19.1, 20.0, 29.0, 28.6] {
        let sec = input::section(350.0, 609.5, dia, &[2], &[2]);
        assert!(sec
            .shape
            .as_ref()
            .is_some_and(|shape| matches!(shape, SectionShape::RcBeamRect { .. })));
        println!(
            "section: dia={dia}, a1={:.9}, A={}, Iy={}, Iz={}",
            one_bar_area(dia),
            sec.area,
            sec.iy,
            sec.iz
        );
        if dia != 19.0 {
            continue;
        }
        let ctx = DesignCtx {
            rebar_material: Some(input::material("SD345", MaterialCategory::Rebar)),
            shear_rebar_material: Some(input::material("SD345", MaterialCategory::Rebar)),
            bond_method: BondMethod::Rc1991,
            length: 4000.0,
            ..Default::default()
        };
        let outcome = RcDesign.check(
            &MemberForcesAt {
                pos: 0.0,
                n: 0.0,
                qy: 20_000.0,
                qz: 0.0,
                mx: 0.0,
                my: 0.0,
                mz: -30e6,
            },
            &sec,
            &input::material("FC24", MaterialCategory::Concrete),
            &ctx,
        );
        let CheckOutcome::Checked(result) = outcome else {
            panic!("{outcome:?}")
        };
        println!("allowable_detail: {}", result.detail);
        for component in result.components {
            if matches!(
                component.kind,
                CheckKind::Bending | CheckKind::Shear | CheckKind::Bond
            ) {
                println!("allowable_component: {component:?}");
            }
        }
    }

    for dia in [19.0, 19.1] {
        let sec = input::section(350.0, 609.5, dia, &[4], &[4]);
        let Some(SectionShape::RcBeamRect { b, d, rebar }) = sec.shape else {
            panic!("RC beam expected")
        };
        let tension = rebar.bending_steel(d, true).tension;
        for a_over_d in [2.0, 3.0] {
            let result = rc_beam_alpha_evaluation(
                b,
                d,
                tension.effective_depth_mm,
                tension.area_mm2,
                RcAlphaSection::Rectangular,
                a_over_d,
                10.0,
                None,
            )
            .unwrap();
            println!("alpha dia={dia} a/D={a_over_d}: {result:?}");
        }
        let src = SectionShape::SrcBeamRect {
            b,
            d,
            rebar,
            steel_height: 400.0,
            steel_width: 200.0,
            steel_web_thick: 8.0,
            steel_flange_thick: 12.0,
        };
        println!(
            "src dia={dia}: {:?}",
            src.src_equivalent_props(24_000.0, 0.2, 10_000.0).unwrap()
        );
    }

    for dia in [10.0, 19.0, 19.1, 20.0, 14.5] {
        let unit_mass = sepika_design_jp::quantity::rebar::rebar_unit_mass_kg_per_m(dia);
        println!("unit mass dia={dia}: {unit_mass} kg/m");
    }

    let a_nominal = 286.5;
    let a_display_diameter = PI * 19.1_f64.powi(2) / 4.0;
    println!("nominal: a={a_nominal}, displayed_diameter_circle={a_display_diameter}");
}
