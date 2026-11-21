#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod beam_input;

use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId};
use sepika_core::model::{
    EndCondition, LoadCase, LoadCaseKind, Material, MaterialCategory, Model, NodalLoad, Section,
};
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
fn beam_equilibrium_and_confirmed_hinges_do_not_verify_the_shear_upper_bound() {
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
        assert_unchecked(check, "ultimate_beam_hinge_shear_upper_bound_unverified:");
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

fn d29_section(width_mm: f64, depth_mm: f64, top: &[u32], bottom: &[u32]) -> Section {
    let mut section = beam_input::section(width_mm, depth_mm, 29.0, top, bottom);
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut section.shape else {
        unreachable!();
    };
    rebar.main_dia = RebarSize::Standard(StandardRebar::D29);
    section.rebar_material = Some(MaterialId(2));
    section
}

fn main_material(grade: &str) -> Material {
    let mut material = beam_input::material(grade, MaterialCategory::Rebar);
    material.id = MaterialId(2);
    material
}

#[test]
fn static_hinge_demand_is_withheld_on_both_sides_of_the_reference_balance_boundary() {
    let mut model = fully_fixed_beam();
    model.sections[0] = d29_section(480.0, 202.0, &[2], &[2]);
    model.materials.push(main_material("SD490"));
    let demand = MemberDemand {
        q_long: Some(0.0),
        static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
        ..Default::default()
    };
    for (fc, reference_q_n, overestimates) in [
        (17.0, 30_538.202, true),
        (18.0, 31_584.359, true),
        (19.0, 32_057.189, true),
        (60.0, 46_067.946, false),
        (72.0, 49_231.634, false),
    ] {
        model.materials[0].fc = Some(fc);
        model.validate().unwrap();
        let result = verify(&model, demand);
        assert!((result.mu - 77_907_060.0).abs() < 1e-6, "{result:?}");
        assert_eq!(38_953.530 > reference_q_n, overestimates);
        let reason = result.unchecked_reason.as_deref().unwrap();
        assert!(reason.contains("i端上端引張") && reason.contains("j端下端引張"));
        assert!(reason.contains("先行する引張主筋の降伏を確認できません"));
        assert_eq!(reason.contains("Fcが通常域の目安18"), fc < 18.0);
        assert_unchecked(result, "ultimate_beam_hinge_shear_upper_bound_unverified:");
    }

    model.materials[0].fc = Some(18.0);
    model.materials[2] = main_material("USD685");
    model.validate().unwrap();
    let result = verify(&model, demand);
    assert!((result.mu - 108_910_890.0).abs() < 1e-6, "{result:?}");
    assert_unchecked(result, "主筋強度が原典記載のSD490級を超えます");
}

#[test]
fn two_percent_tension_ratio_is_reported_without_enabling_static_hinge_demand() {
    let mut model = fully_fixed_beam();
    model.materials[0].fc = Some(18.0);
    model.materials.push(main_material("SD490"));
    let demand = MemberDemand {
        q_long: Some(0.0),
        static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
        ..Default::default()
    };
    for (width_mm, ratio) in [
        (440.0, "2.0069%"),
        (441.512_027_491_408_9, "2.0000%"),
        (450.0, "1.9623%"),
    ] {
        model.sections[0] = d29_section(width_mm, 210.0, &[2], &[2]);
        model.validate().unwrap();
        let result = verify(&model, demand);
        assert!((result.mu - 82_439_834.4).abs() < 1e-6, "{result:?}");
        assert!(result.detail.contains(&format!("a_t/(b d)={ratio}")));
        assert_unchecked(result, "ultimate_beam_hinge_shear_upper_bound_unverified:");
    }
}

#[test]
fn narrow_compression_width_and_unconfirmed_materials_are_reported_without_hinge_demand() {
    let mut model = fully_fixed_beam();
    model.sections[0] = beam_input::section(180.0, 1000.0, 19.0, &[2], &[2]);
    let demand = MemberDemand {
        q_long: Some(0.0),
        static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
        ..Default::default()
    };
    let result = verify(&model, demand);
    assert!(result.mu.is_finite());
    assert!(result.detail.contains("圧縮幅/有効せい=0.191"));
    assert!(result.detail.contains("圧縮幅の適否"));
    assert_unchecked(result, "ultimate_beam_hinge_shear_upper_bound_unverified:");

    let mut unverified_main = main_material("SD345");
    unverified_main.fy = Some(350.0);
    model.materials.push(unverified_main);
    model.sections[0].rebar_material = Some(MaterialId(2));
    model.materials[0].concrete_class = sepika_core::units::ConcreteClass::UserDefined;
    let result = verify(&model, demand);
    assert!(result.mu.is_finite());
    assert!(result
        .detail
        .contains("材料名とfyから原典の通常域に該当する主筋材種を確認できません"));
    assert!(result.detail.contains("普通コンクリートを確認できません"));
    assert_unchecked(result, "ultimate_beam_hinge_shear_upper_bound_unverified:");
}

#[test]
fn asymmetric_beam_returns_directional_reference_mu_and_withholds_static_hinge_demand() {
    let mut model = fully_fixed_beam();
    model.materials[0].fc = Some(18.0);
    model.sections[0] = d29_section(650.0, 214.0, &[3], &[2]);
    model.materials.push(main_material("SD490"));
    model.validate().unwrap();
    for (moment, mechanism, mu_n_mm, section_mu_n_mm) in [
        (
            -10e6,
            StaticBeamHingeMechanism::ITopJBottom,
            127_059_332.4,
            102_416_524.812,
        ),
        (
            10e6,
            StaticBeamHingeMechanism::IBottomJTop,
            84_706_221.6,
            76_223_314.571,
        ),
    ] {
        let result = verify(
            &model,
            MemberDemand {
                mz: moment,
                q_long: Some(0.0),
                static_beam_hinge_mechanism: Some(mechanism),
                ..Default::default()
            },
        );
        assert!((result.mu - mu_n_mm).abs() < 1e-6, "{result:?}");
        assert!(result.mu > section_mu_n_mm);
        let (i_detail, j_detail, side) = if moment < 0.0 {
            (
                "i端上端引張: a_t/(b d)=1.9832%",
                "j端下端引張: a_t/(b d)=1.3222%",
                "上端引張",
            )
        } else {
            (
                "i端下端引張: a_t/(b d)=1.3222%",
                "j端上端引張: a_t/(b d)=1.9832%",
                "下端引張",
            )
        };
        assert!(result.detail.contains(i_detail) && result.detail.contains(j_detail));
        assert_unchecked(result, "ultimate_beam_hinge_bending_unverified:");

        let result = verify(
            &model,
            MemberDemand::from_pushover(0.0, moment, 0.0, 30_000.0, 0.0, 0.0),
        );
        assert!((result.mu - mu_n_mm).abs() < 1e-6, "{result:?}");
        assert_eq!(result.qmu, 30_000.0);
        assert!(result.ok && result.unchecked_reason.is_none(), "{result:?}");
        assert!(result.detail.contains("参考曲げ略算 Mu") && result.detail.contains(side));
        assert!(result
            .detail
            .contains("ultimate_beam_mu_applicability_unverified:"));
    }
}

#[test]
fn static_hinge_shear_is_withheld_when_the_reference_demand_can_underestimate_it() {
    let mut model = fully_fixed_beam();
    model.sections[0] = beam_input::section(350.0, 610.0, 19.0, &[2], &[2]);
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut model.sections[0].shape else {
        unreachable!();
    };
    rebar.main_dia = RebarSize::Standard(StandardRebar::D19);
    model.validate().unwrap();
    let q_long_n = 558_000.0;
    let result = collect_rc_ultimate_checks(
        &model,
        &[(
            ElemId(0),
            MemberDemand {
                q_long: Some(q_long_n),
                static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
                ..Default::default()
            },
        )],
        &UltimateShearOptions {
            include_bond: false,
            ..Default::default()
        },
    )
    .unwrap()
    .remove(0);
    assert!((result.mu - 97_943_033.25).abs() < 1e-6, "{result:?}");
    let old_demand_n = 48_971.516_625;
    let reference_q_n = 54_677.241;
    let remaining_n = result.qsu - q_long_n;
    assert!(
        old_demand_n < remaining_n && remaining_n < reference_q_n,
        "{result:?}"
    );
    assert_unchecked(result, "ultimate_beam_hinge_shear_upper_bound_unverified:");
}

#[test]
fn confirmed_total_shear_is_compared_even_when_reference_mu_cannot_be_calculated() {
    let mut model = fully_fixed_beam();
    model.sections[0] = beam_input::section(350.0, 610.0, 19.0, &[], &[2]);
    let options = UltimateShearOptions {
        include_bond: false,
        ..Default::default()
    };
    for (q_n, ok) in [(30_000.0, true), (1_000_000.0, false)] {
        let result = collect_rc_ultimate_checks(
            &model,
            &[(
                ElemId(0),
                MemberDemand::from_pushover(0.0, -10e6, 0.0, -q_n, 0.0, 0.0),
            )],
            &options,
        )
        .unwrap()
        .remove(0);
        assert_eq!(result.qmu, q_n);
        assert_eq!(result.ok, ok);
        assert_eq!(result.unchecked_reason, None);
        assert!(result.mu.is_nan());
        assert!(result.detail.contains("参考曲げ略算 Muは未算定"));
        assert!(result.detail.contains("上端引張: 引張主筋がありません"));
    }
}

#[test]
fn static_shear_can_fail_on_gravity_response_without_confirming_hinge_demand() {
    let model = fully_fixed_beam();
    let opts = UltimateShearOptions {
        include_bond: false,
        ..Default::default()
    };
    let demand = MemberDemand {
        static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
        ..Default::default()
    };
    let capacity = collect_rc_ultimate_checks(
        &model,
        &[(
            ElemId(0),
            MemberDemand::from_pushover(0.0, -10e6, 0.0, 0.0, 0.0, 0.0),
        )],
        &opts,
    )
    .unwrap()[0]
        .qsu;
    assert!(capacity.is_finite() && capacity > 0.0);
    for (ql, fails) in [(capacity, false), (capacity.next_up(), true)] {
        let result = collect_rc_ultimate_checks(
            &model,
            &[(
                ElemId(0),
                MemberDemand {
                    q_long: Some(-ql),
                    ..demand
                },
            )],
            &opts,
        )
        .unwrap()
        .remove(0);
        assert!(result.qmu.is_nan() && !result.ok);
        if fails {
            assert_eq!(result.shear_margin, 0.0);
            assert_eq!(result.unchecked_reason, None);
            assert!(result.detail.contains("がせん断耐力を超えます"));
        } else {
            assert_unchecked(result, "ultimate_beam_hinge_shear_upper_bound_unverified:");
        }
    }
}

#[test]
fn static_bond_can_fail_on_gravity_response_only_if_both_directions_are_over_capacity() {
    let mut model = fully_fixed_beam();
    model.sections[0] = beam_input::section(400.0, 600.0, 22.0, &[1], &[1]);
    let top = verify(
        &model,
        MemberDemand::from_pushover(0.0, -10e6, 0.0, 0.0, 0.0, 0.0),
    );
    let bottom = verify(
        &model,
        MemberDemand::from_pushover(0.0, 10e6, 0.0, 0.0, 0.0, 0.0),
    );
    assert!(0.0 < top.qbu && top.qbu < bottom.qbu && bottom.qbu < top.qsu);
    for mechanism in [
        StaticBeamHingeMechanism::ITopJBottom,
        StaticBeamHingeMechanism::IBottomJTop,
    ] {
        for (ql, fails) in [
            (top.qbu.next_up(), false),
            (bottom.qbu, false),
            (bottom.qbu.next_up(), true),
        ] {
            let result = verify(
                &model,
                MemberDemand {
                    q_long: Some(ql),
                    static_beam_hinge_mechanism: Some(mechanism),
                    ..Default::default()
                },
            );
            assert!(result.qmu.is_nan() && !result.ok);
            if fails {
                assert_eq!(result.bond_margin, 0.0);
                assert!(result.shear_margin.is_nan());
                assert_eq!(result.unchecked_reason, None);
                assert!(result.detail.contains("双方の付着耐力を超えます"));
            } else {
                assert_unchecked(result, "ultimate_beam_hinge_shear_upper_bound_unverified:");
            }
        }
    }
}
