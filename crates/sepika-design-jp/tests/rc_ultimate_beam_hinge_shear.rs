#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod beam;
#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod column;

use sepika_core::adjacency::NodeAdjacency;
use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::face_distance::girder_column_faces;
use sepika_core::ids::{ElemId, NodeId, SectionId};
use sepika_core::model::{Constraint, EndCondition, Model};
use sepika_core::rc_capacity::RcUltimateShearActions;
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::ultimate::{
    collect_rc_ultimate_checks, MemberDemand, RcUltimateBeamHingeShearChecks,
    RcUltimateBeamHingeShearDemand, RcUltimateBeamHingeShearDirectionDemand, StaticBeamBending,
    StaticBeamHingeMechanism, UltimateCheck, UltimateShearOptions,
};
use sepika_solver::statics::analysis::SeismicDir;

fn model() -> Model {
    let mut model = beam::model(true);
    model.ultimate_gravity_combination = Some(0);
    model.nodes[1].coord[0] = 4600.0;
    model.elements[0].local_axis.ref_vector = [0.0, 0.0, 1.0];
    let column = column::column_model(false, 0.0);
    for (side, width_mm) in [(0, 400.0), (1, 800.0)] {
        let id = side + 1;
        let template = &column.sections[0];
        let mut shape = template.shape.clone().unwrap();
        let SectionShape::RcColumnRect { b, .. } = &mut shape else {
            unreachable!();
        };
        *b = width_mm;
        let mut section = shape.to_section(SectionId(id), format!("C{id}"));
        section.material = template.material;
        section.rebar_material = template.rebar_material;
        section.shear_rebar_material = template.shear_rebar_material;
        section.frame_use = template.frame_use;
        model.sections.push(section);

        let mut bottom = column.nodes[0].clone();
        bottom.id = NodeId(side + 2);
        bottom.coord = [model.nodes[side as usize].coord[0], 0.0, -3000.0];
        model.nodes.push(bottom);

        let mut support = column.elements[0].clone();
        support.id = ElemId(id);
        support.nodes = [NodeId(side + 2), NodeId(side)].into_iter().collect();
        support.section = Some(SectionId(id));
        support.local_axis.ref_vector = [0.0, 1.0, 0.0];
        model.elements.push(support);
    }
    model.validate().unwrap();
    model
}

fn input() -> RcUltimateBeamHingeShearDemand {
    RcUltimateBeamHingeShearDemand {
        seismic_axis: SeismicDir::X,
        face_positions_mm: [200.0, 4200.0],
        q0_n: [30_000.0, -90_000.0],
        end_envelope_confirmed: true,
        positive: Some(RcUltimateBeamHingeShearDirectionDemand {
            mechanism: StaticBeamHingeMechanism::ITopJBottom,
            yield_moment_bounds_nmm: [140e6, 220e6],
            support_transfer_confirmed: true,
        }),
        negative: Some(RcUltimateBeamHingeShearDirectionDemand {
            mechanism: StaticBeamHingeMechanism::IBottomJTop,
            yield_moment_bounds_nmm: [200e6, 80e6],
            support_transfer_confirmed: true,
        }),
    }
}

fn static_demand() -> MemberDemand {
    MemberDemand {
        q_long: Some(1000.0),
        ..MemberDemand::axial(0.0)
    }
}

fn check(model: &Model, beam_demand: MemberDemand) -> UltimateCheck {
    let demands: Vec<_> = model
        .elements
        .iter()
        .map(|elem| {
            (
                elem.id,
                if elem.id == ElemId(0) {
                    beam_demand
                } else {
                    static_demand()
                },
            )
        })
        .collect();
    collect_rc_ultimate_checks(
        model,
        &demands,
        &UltimateShearOptions {
            include_bond: false,
            ..Default::default()
        },
    )
    .unwrap()
    .into_iter()
    .find(|check| check.elem == ElemId(0))
    .unwrap()
}

fn direction_checks(
    model: &Model,
    input: RcUltimateBeamHingeShearDemand,
) -> RcUltimateBeamHingeShearChecks {
    check(
        model,
        MemberDemand {
            beam_hinge_shear: Some(input),
            ..static_demand()
        },
    )
    .beam_hinge_shear
    .unwrap()
}

#[test]
fn verified_directions_use_both_unequal_face_moments_and_signed_gravity() {
    let mut model = model();
    model.elements[0].rigid_zone.face_i = Some(2800.0);
    model.elements[0].rigid_zone.face_j = Some(2200.0);
    for (long_mz, bending) in [
        (40e6, StaticBeamBending::Signed),
        (-40e6, StaticBeamBending::Mixed),
    ] {
        let result = check(
            &model,
            MemberDemand {
                mz: long_mz,
                static_beam_bending: bending,
                beam_hinge_shear: Some(input()),
                ..static_demand()
            },
        );
        assert!(result.qmu.is_nan() && result.qsu.is_nan() && !result.ok);
        assert!(result
            .detail
            .contains("X軸正の両端降伏枝（従来の比較とは別）"));
        assert!(result.detail.contains("Q0(i/j)=30000.000/-90000.000 N"));
        assert!(result
            .detail
            .contains("曲げ上限を用いたQy=90000.000 N, QD2(i/j)=120000.000/0.000 N"));
        let directions = result.beam_hinge_shear.unwrap();
        assert_eq!(directions.seismic_axis, SeismicDir::X);
        let positive = directions.positive.unwrap();
        let negative = directions.negative.unwrap();

        // 4 m の自由体。常時荷重は i から3 mに下向き120 kN、反力は i が30 kN、j が90 kN。
        // 端モーメントの不釣合い 360/280 kN·m が 90/-70 kN の逆向き反力対を生じる。
        assert_eq!(positive.face_positions_mm, [200.0, 4200.0]);
        assert_eq!(positive.clear_span_mm, 4000.0);
        assert_eq!(positive.yield_mz_at_upper_bound_nmm, [-140e6, 220e6]);
        assert_eq!(positive.q0_n, [30_000.0, -90_000.0]);
        assert_eq!(positive.qy_at_upper_bound_n, 90_000.0);
        assert_eq!(positive.qd2_at_upper_bound_n, [120_000.0, 0.0]);
        assert_eq!(positive.max_abs_qd2_bound_n, 120_000.0);
        assert_eq!(negative.yield_mz_at_upper_bound_nmm, [200e6, -80e6]);
        assert_eq!(negative.qy_at_upper_bound_n, -70_000.0);
        assert_eq!(negative.qd2_at_upper_bound_n, [-40_000.0, -160_000.0]);
        assert_eq!(negative.max_abs_qd2_bound_n, 160_000.0);
    }
}

#[test]
fn moment_bounds_do_not_understate_demand_when_gravity_shear_cancels() {
    let mut demand = input();
    demand.q0_n = [1000.0, -100_000.0];
    demand.positive.as_mut().unwrap().yield_moment_bounds_nmm = [20e6, 24e6];
    let model = model();
    let bound = direction_checks(&model, demand).positive.unwrap();
    assert_eq!(bound.qy_at_upper_bound_n, 11_000.0);
    assert_eq!(bound.qd2_at_upper_bound_n, [12_000.0, -89_000.0]);

    demand.positive.as_mut().unwrap().yield_moment_bounds_nmm = [20e6, 20e6];
    let smaller = direction_checks(&model, demand).positive.unwrap();
    assert_eq!(smaller.qd2_at_upper_bound_n, [11_000.0, -90_000.0]);
    assert_eq!(smaller.max_abs_qd2_bound_n, 100_000.0);
    assert_eq!(bound.max_abs_qd2_bound_n, 100_000.0);
}

#[test]
fn missing_or_invalid_direction_does_not_hide_the_other_direction() {
    let model = model();
    let valid = input();
    let errors = [
        (None, "この方向の両端曲げ降伏"),
        (
            Some(RcUltimateBeamHingeShearDirectionDemand {
                support_transfer_confirmed: false,
                ..valid.positive.unwrap()
            }),
            "支持・接合",
        ),
        (
            Some(RcUltimateBeamHingeShearDirectionDemand {
                yield_moment_bounds_nmm: [0.0, 2e8],
                ..valid.positive.unwrap()
            }),
            "i端",
        ),
        (
            Some(RcUltimateBeamHingeShearDirectionDemand {
                yield_moment_bounds_nmm: [2e8, -1.0],
                ..valid.positive.unwrap()
            }),
            "j端",
        ),
        (
            Some(RcUltimateBeamHingeShearDirectionDemand {
                yield_moment_bounds_nmm: [2e8, f64::NAN],
                ..valid.positive.unwrap()
            }),
            "j端",
        ),
        (
            Some(RcUltimateBeamHingeShearDirectionDemand {
                yield_moment_bounds_nmm: [f64::INFINITY, 2e8],
                ..valid.positive.unwrap()
            }),
            "i端",
        ),
        (
            Some(RcUltimateBeamHingeShearDirectionDemand {
                yield_moment_bounds_nmm: [f64::from_bits(1); 2],
                ..valid.positive.unwrap()
            }),
            "表現範囲外",
        ),
    ];
    for (positive, message) in errors {
        let result = direction_checks(&model, RcUltimateBeamHingeShearDemand { positive, ..valid });
        let reason = result.positive.unwrap_err();
        assert!(reason.contains(message), "{reason}");
        assert_eq!(
            result.negative.unwrap().qd2_at_upper_bound_n,
            [-40_000.0, -160_000.0]
        );
    }
}

#[test]
fn verified_faces_and_signed_gravity_envelope_are_required_for_both_directions() {
    let model = model();
    let valid = input();
    for (demand, message) in [
        (
            RcUltimateBeamHingeShearDemand {
                end_envelope_confirmed: false,
                ..valid
            },
            "両端の Q0 の範囲内",
        ),
        (
            RcUltimateBeamHingeShearDemand {
                q0_n: [f64::MAX, f64::NAN],
                ..valid
            },
            "Q0 が非有限",
        ),
        (
            RcUltimateBeamHingeShearDemand {
                face_positions_mm: [300.0, 4200.0],
                ..valid
            },
            "i端",
        ),
        (
            RcUltimateBeamHingeShearDemand {
                face_positions_mm: [200.0, f64::INFINITY],
                ..valid
            },
            "j端",
        ),
    ] {
        let result = direction_checks(&model, demand);
        for direction in [result.positive, result.negative] {
            let reason = direction.unwrap_err();
            assert!(reason.contains(message), "{reason}");
        }
    }
}

#[test]
fn model_disqualifications_override_designer_confirmation() {
    let assert_rejected = |model: &Model, message| {
        let result = direction_checks(model, input());
        for direction in [result.positive, result.negative] {
            let reason = direction.unwrap_err();
            assert!(reason.contains(message), "{reason}");
        }
    };
    let original = model();
    let mut model = original.clone();
    model.elements[0].end_cond[0] = EndCondition::Pinned;
    assert_rejected(&model, "i端が曲げ解放");
    model = original.clone();
    model.elements[0].end_cond[1] = EndCondition::SemiRigid { k_theta: 0.0 };
    assert_rejected(&model, "j端が曲げ解放");
    model = original.clone();
    model.elements[2].end_cond[1] = EndCondition::Pinned;
    assert_rejected(&model, "j端の接続柱に曲げを伝達できる端部がありません");
    model = original.clone();
    model.elements.pop();
    for restraint in [Dof6Mask::FREE, Dof6Mask::PINNED] {
        model.nodes[1].restraint = restraint;
        assert_rejected(&model, "j端の実柱面: 接続柱がありません");
    }
    model = original.clone();
    model.nodes[1].coord[0] = 500.0;
    model.nodes[3].coord[0] = 500.0;
    assert_rejected(&model, "接続柱の側面間に梁区間がありません");
    model = original;
    model.girder_groups.push(vec![ElemId(0), ElemId(1)]);
    assert_rejected(&model, "複数要素");
}

#[test]
fn optional_hinge_demand_does_not_replace_confirmed_total_shear_or_its_failure() {
    let model = model();
    let total = MemberDemand {
        rc_ultimate_shear: Some(RcUltimateShearActions {
            axial_range_n: [0.0; 2],
            max_mq: [(200e6, 1e6), (0.0, 0.0)],
        }),
        ..MemberDemand::from_pushover(0.0, 200e6, 0.0, 1e6, 0.0, 0.0)
    };
    let baseline = check(&model, total);
    let combined = check(
        &model,
        MemberDemand {
            beam_hinge_shear: Some(input()),
            ..total
        },
    );
    assert!(baseline.beam_hinge_shear.is_none());
    assert_eq!(
        combined
            .beam_hinge_shear
            .unwrap()
            .positive
            .unwrap()
            .qd2_at_upper_bound_n,
        [120_000.0, 0.0]
    );
    assert_eq!(combined.qmu, 1e6);
    assert_eq!(combined.qmu, baseline.qmu);
    assert_eq!(combined.qsu, baseline.qsu);
    assert_eq!(combined.shear_margin, baseline.shear_margin);
    assert!(combined.shear_margin < 1.0);
    assert!(!combined.ok && combined.unchecked_reason.is_none());
}

#[test]
fn exact_model_face_positions_are_required_even_at_the_limit_of_a_positive_clear_span() {
    let mut model = model();
    let mut requested = input();
    requested.face_positions_mm[1] = requested.face_positions_mm[1].next_up();
    let reason = direction_checks(&model, requested).positive.unwrap_err();
    assert!(
        reason.contains("j端") && reason.contains("一致しません"),
        "{reason}"
    );

    let x_j_mm = 600.0_f64.next_up();
    model.nodes[1].coord[0] = x_j_mm;
    model.nodes[3].coord[0] = x_j_mm;
    let faces = girder_column_faces(
        &model,
        &model.elements[0],
        &NodeAdjacency::primary_frame(&model),
    )
    .unwrap()
    .map(|face| face.unwrap().position_mm);
    assert!(0.0 < faces[1] - faces[0] && faces[1] - faces[0] < 1e-10);
    requested.face_positions_mm = faces;
    assert!(direction_checks(&model, requested).positive.is_ok());

    for invalid in [[faces[0]; 2], [faces[1], faces[0]]] {
        requested.face_positions_mm = invalid;
        let result = direction_checks(&model, requested);
        for direction in [result.positive, result.negative] {
            let reason = direction.unwrap_err();
            assert!(
                reason.contains("入力の実支持フェース位置に有限で正の内法が成立しません"),
                "{reason}"
            );
        }
    }
}

fn assert_j_end_cannot_transfer_moment(model: &Model) {
    let result = direction_checks(model, input());
    for direction in [result.positive, result.negative] {
        let reason = direction.unwrap_err();
        assert!(
            reason.contains("j端の強軸曲げを受ける直接支持を確認できません"),
            "{reason}"
        );
    }
}

#[test]
fn the_column_at_the_face_must_have_direct_support_or_the_beam_joint_must_resist_moment() {
    let mut model = model();
    model.nodes[3].restraint = Dof6Mask::FREE;
    assert_j_end_cannot_transfer_moment(&model);

    model.nodes[3].support_spring = Some([5_000.0; 6]);
    assert!(direction_checks(&model, input()).positive.is_ok());
    model.nodes[3].support_spring = None;
    model.nodes[1].restraint = Dof6Mask::FIXED;
    assert!(direction_checks(&model, input()).positive.is_ok());

    model.nodes[1].restraint = Dof6Mask::FREE;
    let mut lower_node = model.nodes[2].clone();
    lower_node.id = NodeId(4);
    lower_node.coord = [4600.0, 0.0, -6000.0];
    model.nodes.push(lower_node);
    let mut lower_column = model.elements[2].clone();
    lower_column.id = ElemId(3);
    lower_column.nodes = [NodeId(4), NodeId(3)].into_iter().collect();
    model.elements.push(lower_column);
    model.validate().unwrap();
    assert_j_end_cannot_transfer_moment(&model);
}

#[test]
fn a_rigid_link_that_only_couples_translation_at_the_joint_does_not_supply_direct_support() {
    let mut model = model();
    model.nodes[3].restraint = Dof6Mask::FREE;
    let mut anchor = model.nodes[1].clone();
    anchor.id = NodeId(4);
    anchor.restraint = Dof6Mask::FIXED;
    model.nodes.push(anchor);
    let mut dofs = Dof6Mask::FREE;
    dofs.set_fixed(Dof::Ux);
    model.constraints.push(Constraint::RigidLink {
        master: NodeId(4),
        slaves: vec![NodeId(1)],
        dofs,
    });
    model.validate().unwrap();
    assert_j_end_cannot_transfer_moment(&model);
}

#[test]
fn direct_support_at_the_beam_joint_must_restrain_strong_axis_rotation() {
    let mut model = model();
    model.nodes[3].restraint = Dof6Mask::FREE;

    model.nodes[1].restraint = Dof6Mask::PINNED;
    assert_j_end_cannot_transfer_moment(&model);
    model.nodes[1].restraint = Dof6Mask::FREE;
    model.nodes[1].support_spring = Some([5_000.0, 5_000.0, 5_000.0, 0.0, 0.0, 0.0]);
    assert_j_end_cannot_transfer_moment(&model);
    model.nodes[1].restraint.set_fixed(Dof::Rz);
    assert_j_end_cannot_transfer_moment(&model);

    model.nodes[1].restraint = Dof6Mask::FREE;
    model.nodes[1].restraint.set_fixed(Dof::Ry);
    assert!(direction_checks(&model, input()).positive.is_ok());
}

#[test]
fn a_released_column_far_end_needs_the_translation_that_resists_beam_bending() {
    let mut model = model();
    model.nodes[3].restraint = Dof6Mask::FREE;
    for condition in [
        EndCondition::Pinned,
        EndCondition::SemiRigid { k_theta: 0.0 },
    ] {
        model.elements[2].end_cond[0] = condition;
        model.nodes[3].support_spring = Some([0.0, 0.0, 0.0, 0.0, 5_000.0, 0.0]);
        assert_j_end_cannot_transfer_moment(&model);
        model.nodes[3].support_spring.as_mut().unwrap()[1] = 5_000.0;
        assert_j_end_cannot_transfer_moment(&model);
        model.nodes[3].support_spring.as_mut().unwrap()[0] = 5_000.0;
        assert!(direction_checks(&model, input()).positive.is_ok());
    }
}

#[test]
fn design_envelope_rounds_up_without_changing_the_signed_face_values() {
    let mut model = model();
    model.nodes[1].coord[0] = 603.0;
    model.nodes[3].coord[0] = 603.0;
    let mut demand = input();
    demand.face_positions_mm = [200.0, 203.0];
    demand.q0_n = [0.0; 2];
    demand.positive.as_mut().unwrap().yield_moment_bounds_nmm = [0.5; 2];
    let check = check(
        &model,
        MemberDemand {
            beam_hinge_shear: Some(demand),
            ..static_demand()
        },
    );
    assert!(check.detail.contains("参考表示（丸めあり） Q0(i/j)=0.000/0.000 N, 曲げ上限を用いたQy=0.333 N, QD2(i/j)=0.333/0.333 N, 絶対包絡上限=0.333 N"));
    let result = check.beam_hinge_shear.unwrap().positive.unwrap();
    assert_eq!(result.qd2_at_upper_bound_n, [0.333_333_333_333_333_3; 2]);
    assert_eq!(result.max_abs_qd2_bound_n, 0.333_333_333_333_333_37);
}
