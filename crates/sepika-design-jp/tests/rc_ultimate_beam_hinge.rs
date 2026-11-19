#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod beam_input;

use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::{ElemId, LoadCaseId, NodeId};
use sepika_core::model::{EndCondition, LoadCase, LoadCaseKind, Model, NodalLoad};
use sepika_core::section_shape::{RebarSize, SectionShape, StandardRebar};
use sepika_design_jp::ultimate::{
    collect_rc_ultimate_checks, MemberDemand, StaticBeamHingeMechanism, UltimateCheck,
    UltimateShearOptions,
};
use sepika_solver::statics::linear::linear_static_once;

fn fully_fixed_beam() -> Model {
    let mut model = beam_input::model(false);
    model.nodes[1].coord[0] = 4_000.0;
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.load_cases[0].nodal.clear();
    model.ultimate_gravity_combination = Some(0);
    model.elements[0].rigid_zone.face_i = Some(0.0);
    model.elements[0].rigid_zone.face_j = Some(0.0);
    model.sections[0] = beam_input::section(350.0, 609.5, 19.0, &[2], &[2]);
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut model.sections[0].shape else {
        unreachable!();
    };
    rebar.main_dia = RebarSize::Standard(StandardRebar::D19);
    model
}

fn verify(model: &Model, demand: MemberDemand) -> UltimateCheck {
    collect_rc_ultimate_checks(
        model,
        &[(ElemId(0), demand)],
        &UltimateShearOptions::default(),
    )
    .unwrap()
    .remove(0)
}

fn assert_unchecked(check: UltimateCheck, code: &str) {
    assert!(!check.ok);
    assert!(check.qmu.is_nan() && check.shear_margin.is_nan() && check.bond_margin.is_nan());
    let reason = check.unchecked_reason.unwrap();
    assert!(
        reason.contains(code) && reason.contains("member=0"),
        "{reason}"
    );
}

#[test]
fn beam_end_support_is_required_even_with_a_confirmed_hinge_mechanism() {
    let demand = MemberDemand {
        mz: -10e6,
        q_long: Some(2_500.0),
        static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
        ..Default::default()
    };
    for (restraint, condition, reason) in [
        (
            Dof6Mask::FREE,
            EndCondition::Fixed,
            "ultimate_beam_hinge_free_end: j端",
        ),
        (
            Dof6Mask::FIXED,
            EndCondition::Pinned,
            "ultimate_beam_hinge_end_released: j端",
        ),
        (
            Dof6Mask::PINNED,
            EndCondition::Fixed,
            "ultimate_beam_hinge_end_released: j端",
        ),
        (
            Dof6Mask::FIXED,
            EndCondition::SemiRigid { k_theta: 0.0 },
            "ultimate_beam_hinge_end_released: j端",
        ),
        (
            Dof6Mask::FIXED,
            EndCondition::SemiRigid { k_theta: 1e12 },
            "ultimate_beam_hinge_support_unverified: j端",
        ),
    ] {
        let mut model = fully_fixed_beam();
        model.nodes[1].restraint = restraint;
        model.elements[0].end_cond[1] = condition;
        assert_unchecked(verify(&model, demand), reason);
    }
}

#[test]
fn beam_end_restraints_and_gravity_moment_cannot_verify_the_hinge_mechanism() {
    let model = fully_fixed_beam();
    for (mz, q_long) in [(-10e6, 2_500.0), (10e6, 1_000_000.0)] {
        let check = verify(
            &model,
            MemberDemand {
                mz,
                q_long: Some(q_long),
                ..Default::default()
            },
        );
        assert!(check.mu.is_finite() && check.qsu.is_finite() && check.qbu.is_finite());
        assert_unchecked(check, "ultimate_beam_hinge_mechanism_unverified:");
    }
}

#[test]
fn a_connected_end_without_confirmed_rotation_restraint_is_support_unverified() {
    let mut model = fully_fixed_beam();
    model.nodes[1].restraint = Dof6Mask::FREE;
    model.nodes.push(sepika_core::model::Node {
        id: NodeId(2),
        coord: [4_000.0, 0.0, -3_000.0],
        ..model.nodes[0].clone()
    });
    let mut adjacent = model.elements[0].clone();
    adjacent.id = ElemId(1);
    adjacent.nodes = [NodeId(1), NodeId(2)].into_iter().collect();
    adjacent.section = None;
    model.elements.push(adjacent);
    let demand = MemberDemand {
        mz: -10e6,
        q_long: Some(2_500.0),
        static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
        ..Default::default()
    };
    assert_unchecked(
        verify(&model, demand),
        "ultimate_beam_hinge_support_unverified: j端",
    );
}

#[test]
fn confirmed_symmetric_beam_hinge_demand_matches_the_loaded_beam_equilibrium() {
    let mut model = fully_fixed_beam();
    model.nodes[1].restraint.set_free(Dof::Uy);
    let lateral_case = LoadCaseId(2);
    model.load_cases.push(LoadCase {
        id: lateral_case,
        name: "j端強軸方向横力".into(),
        kind: LoadCaseKind::Seismic,
        origin: Default::default(),
        nodal: vec![NodalLoad::manual(NodeId(1), [0.0; 6])],
        member: vec![],
    });
    let mu_n_mm = 97_854_075.0;
    let shear_n = 48_927.037_5;
    // 区間荷重のない一様梁の両端の回転を拘束すると、j端の横力Pに対し
    // i端の反力は−P、梁の強軸せん断は一定、両端の断面曲げは±PL/2となる。
    for (direction, mechanism) in [
        (1.0, StaticBeamHingeMechanism::IBottomJTop),
        (-1.0, StaticBeamHingeMechanism::ITopJBottom),
    ] {
        let p_n = direction * shear_n;
        model.load_cases[lateral_case.index()].nodal[0].values[1] = p_n;
        let solved = linear_static_once(&model, lateral_case).unwrap();
        assert_eq!(solved.disp[0], [0.0; 6]);
        assert!(direction * solved.disp[1][1] > 0.0);
        assert_eq!(solved.disp[1][3..], [0.0; 3]);
        let [(id, forces)] = solved.member_forces.as_slice() else {
            panic!("1本の梁の内力が必要です");
        };
        assert_eq!(*id, ElemId(0));
        let (xi_i, i) = forces.at.first().unwrap();
        let (xi_j, j) = forces.at.last().unwrap();
        assert_eq!((*xi_i, *xi_j), (0.0, 1.0));
        for (_, force) in &forces.at {
            assert!((force[1] + p_n).abs() < 1e-6);
        }
        assert!((i[5] - direction * mu_n_mm).abs() < 1e-6);
        assert!((j[5] + direction * mu_n_mm).abs() < 1e-6);

        let demand = MemberDemand {
            q_long: Some(0.0),
            ..Default::default()
        };
        assert_unchecked(
            verify(&model, demand),
            "ultimate_beam_hinge_mechanism_unverified:",
        );
        let check = verify(
            &model,
            MemberDemand {
                static_beam_hinge_mechanism: Some(mechanism),
                ..demand
            },
        );
        assert!((check.mu - mu_n_mm).abs() < 1e-6);
        assert!((check.qmu - shear_n).abs() < 1e-6);
        assert!(check.ok && check.unchecked_reason.is_none(), "{check:?}");
    }
}

#[test]
fn independently_confirmed_hinges_do_not_enable_the_symmetric_shortcut_for_asymmetric_beams() {
    let mut model = fully_fixed_beam();
    model.sections[0] = beam_input::section(350.0, 609.5, 19.0, &[2], &[4]);
    for (moment, mechanism) in [
        (-10e6, StaticBeamHingeMechanism::ITopJBottom),
        (10e6, StaticBeamHingeMechanism::IBottomJTop),
    ] {
        assert_unchecked(
            verify(
                &model,
                MemberDemand {
                    mz: moment,
                    static_beam_hinge_mechanism: Some(mechanism),
                    q_long: Some(0.0),
                    ..Default::default()
                },
            ),
            "ultimate_beam_hinge_bending_unverified:",
        );
    }
}
