#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod beam_input;

use sepika_core::{
    dof::Dof6Mask,
    ids::{ElemId, MaterialId},
    model::{MaterialCategory, Model},
};
use sepika_design_jp::ultimate::{
    collect_rc_ultimate_checks, MemberDemand, StaticBeamHingeMechanism, UltimateShearOptions,
};

const GRAVITY_SHEAR_N: f64 = 12_000.0;

fn static_beam() -> (Model, [(ElemId, MemberDemand); 1]) {
    let mut model = beam_input::model(false);
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.ultimate_gravity_combination = Some(0);
    let elem = &mut model.elements[0];
    elem.rigid_zone.face_i = Some(0.0);
    elem.rigid_zone.face_j = Some(0.0);
    let demand = MemberDemand {
        mz: 1.0e7,
        static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::IBottomJTop),
        q_long: Some(GRAVITY_SHEAR_N),
        ..Default::default()
    };
    let input = [(elem.id, demand)];
    (model, input)
}

#[test]
fn static_hinge_rejects_nonpositive_and_nonfinite_strength_factors() {
    let (model, input) = static_beam();
    for factor in [0.0, -0.0, -0.5, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let opts = UltimateShearOptions {
            upper_strength_factor: factor,
            ..Default::default()
        };
        let reason = collect_rc_ultimate_checks(&model, &input, &opts).unwrap_err();
        assert!(
            reason.contains("ultimate_upper_strength_factor_invalid:"),
            "{reason}"
        );
        assert!(reason.contains("member=0"), "{reason}");
    }
}

#[test]
fn static_hinge_positive_strength_factors_scale_demand_and_margins() {
    let (model, input) = static_beam();
    let unit = collect_rc_ultimate_checks(&model, &input, &UltimateShearOptions::default())
        .unwrap()
        .remove(0);
    // SD345・主筋3本（径19 mm）・有効せい290.5 mm・内法3000 mmの両端ヒンジ需要。
    assert!((unit.qmu - 51_148.725_917).abs() < 1e-6);
    let unit_shear_margin = (unit.qsu - GRAVITY_SHEAR_N) / unit.qmu;
    let unit_bond_margin = (unit.qbu - GRAVITY_SHEAR_N) / unit.qmu;
    assert!(unit_shear_margin > 0.0 && unit_bond_margin > 0.0);

    for factor in [0.5, 1.0, 1.5] {
        let opts = UltimateShearOptions {
            upper_strength_factor: factor,
            ..Default::default()
        };
        let check = collect_rc_ultimate_checks(&model, &input, &opts)
            .unwrap()
            .remove(0);
        assert_eq!(check.unchecked_reason, None);
        assert_eq!(check.mu, unit.mu);
        assert_eq!(check.qsu, unit.qsu);
        assert_eq!(check.qbu, unit.qbu);
        assert!((check.qmu / unit.qmu - factor).abs() < 1e-12);
        assert!((check.shear_margin - unit_shear_margin / factor).abs() < 1e-12);
        assert!((check.bond_margin - unit_bond_margin / factor).abs() < 1e-12);
    }
}

#[test]
fn static_beam_compares_both_bond_capacities_when_hinge_demand_underflows() {
    let min_factor = f64::from_bits(1);
    let (mut model, mut input) = static_beam();
    model.sections[0] = beam_input::section(400.0, 600.0, 22.0, &[1], &[1]);
    model.materials[1] = beam_input::material("SD295", MaterialCategory::Rebar);
    let opts = UltimateShearOptions {
        upper_strength_factor: min_factor,
        ..Default::default()
    };
    let standard = &collect_rc_ultimate_checks(&model, &input, &opts).unwrap()[0];
    assert!(standard.ok && standard.qmu > 0.0);

    // 3 m の梁で Qmu の丸めを再現するため、主筋だけを試験用の極小降伏強度にする。
    model.materials.push(sepika_core::model::Material {
        id: MaterialId(2),
        name: "試験用主筋".into(),
        fy: Some(1.0e-4),
        ..model.materials[1].clone()
    });
    model.sections[0].rebar_material = Some(MaterialId(2));
    model.validate().unwrap();

    // 確定総応答で上端引張を指定し、反対側耐力と等しい長期せん断力を入力する。
    let opposite = &collect_rc_ultimate_checks(
        &model,
        &[(
            input[0].0,
            MemberDemand::from_pushover(0.0, -1.0e7, 0.0, 0.0, 0.0, 0.0),
        )],
        &UltimateShearOptions {
            upper_strength_factor: 1.0,
            ..opts.clone()
        },
    )
    .unwrap()[0];
    let capacity_n = opposite.qbu;
    assert!(capacity_n.is_finite() && capacity_n > 0.0);
    input[0].1.q_long = Some(-capacity_n);
    let at_capacity = &collect_rc_ultimate_checks(&model, &input, &opts).unwrap()[0];
    assert!(at_capacity.mu > 0.0);
    assert_eq!(at_capacity.qmu, 0.0);
    assert_eq!(at_capacity.unchecked_reason, None);
    assert!(at_capacity.ok);
    assert!(at_capacity.qsu > capacity_n);
    assert!(opposite.qsu > capacity_n.next_up() && at_capacity.qbu > capacity_n);

    input[0].1.q_long = Some(-capacity_n.next_up());
    let above_capacity = &collect_rc_ultimate_checks(&model, &input, &opts).unwrap()[0];
    assert!(!above_capacity.ok);
    let reason = above_capacity.unchecked_reason.as_deref().unwrap();
    assert!(
        reason.contains("ultimate_beam_hinge_bond_unverified:"),
        "{reason}"
    );
}
