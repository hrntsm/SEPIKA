use super::*;
use crate::common::constraint::Reducer;
use sepika_core::dof::{Dof6Mask, DofMap};
use sepika_core::ids::{NodeId, StoryId};
use sepika_core::model::{
    Constraint, ElementKind, LoadCase, MemberLoad, MemberLoadKind, Model, NodalLoad,
};
use sepika_element::behavior::{Ctx, LocalVec};

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-5,
        "actual={actual} expected={expected}"
    );
}

#[test]
fn equal_frame_and_ground_spring_keep_total_resistance_distinct_from_frame_cut() {
    let mut model = super::tests::single_column_model(1e8, 80_000.0);
    model.elements[0].kind = ElementKind::Fiber;
    model.sections[0].as_y = 10000.;
    model.sections[0].as_z = 10000.;
    model.materials[0].shear = Some(80000.);
    let stiffness = 1. / (3000_f64.powi(3) / (3. * 205000. * 8.333e6) + 3000. / (80000. * 10000.));
    model.nodes[1].support_spring = Some([stiffness, 0., 0., 0., 0., 0.]);
    let map = DofMap::build(&model);
    let reducer = Reducer::build(&model, &map);
    let result = pushover_analysis(&model, &map, &reducer, SeismicDir::X, 1, 0., false).unwrap();
    let point = result.capacity_curve.last().unwrap();
    close(point.base_shear, 16000.);
    close(point.story_shear[0], 16000.);
    close(stiffness * point.roof_disp, 8000.);
    let cut = &result
        .confirmed_history
        .as_ref()
        .unwrap()
        .last()
        .unwrap()
        .cuts[0];
    close(cut.support_n, 8000.);
    close(cut.evaluate().unwrap().qu_n, 8000.);
}

#[test]
fn diaphragm_master_load_and_direct_slave_load_have_the_same_force_and_work() {
    for direction in [SeismicDir::X, SeismicDir::Y] {
        let d = usize::from(direction == SeismicDir::Y);
        let mut model = super::tests::single_column_model(1e8, 80_000.);
        model.sections[0].as_y = 10000.;
        model.sections[0].as_z = 10000.;
        let mut master = model.nodes[1].clone();
        master.id = NodeId(2);
        master.restraint = Dof6Mask(0b111100);
        master.story = Some(StoryId(1));
        model.nodes.push(master);
        let direct_map = DofMap::build(&model);
        let direct_reducer = Reducer::build(&model, &direct_map);
        let direct = pushover_analysis(
            &model,
            &direct_map,
            &direct_reducer,
            direction,
            1,
            0.,
            false,
        )
        .unwrap();
        model.constraints.push(Constraint::RigidDiaphragm {
            story: StoryId(1),
            master: NodeId(2),
            slaves: vec![NodeId(1)],
            weight: Some(80_000.),
            ci_override: None,
        });
        let map = DofMap::build(&model);
        let reducer = Reducer::build(&model, &map);
        let master_result =
            pushover_analysis(&model, &map, &reducer, direction, 1, 0., false).unwrap();
        let point = master_result.capacity_curve.last().unwrap();
        close(point.base_shear, 16000.);
        close(point.story_shear[0], 16000.);
        let cut = &master_result
            .confirmed_history
            .as_ref()
            .unwrap()
            .last()
            .unwrap()
            .cuts[0];
        close(cut.reference_n, 16000.);
        close(cut.evaluate().unwrap().qu_n, 16000.);
        close(
            point.base_shear * point.roof_disp,
            direct.capacity_curve.last().unwrap().base_shear
                * direct.capacity_curve.last().unwrap().roof_disp,
        );
        assert!(model.stories[1].node_ids.iter().all(|n| *n != NodeId(2)));
        assert!(map.active(12 + d).is_some());
    }
}

#[test]
fn fixed_loaded_member_keeps_nonzero_total_resistance_with_zero_displacement() {
    for reversed in [false, true] {
        let mut model = super::tests::single_column_model(1e8, 80_000.);
        model.nodes[1].restraint = Dof6Mask::FIXED;
        if reversed {
            model.elements[0].nodes.reverse();
        }
        let load = MemberLoad::manual(
            model.elements[0].id,
            [1., 0., 0.],
            MemberLoadKind::Distributed {
                a: 0.,
                b: 3000.,
                w1: 2.,
                w2: 2.,
            },
        );
        model.load_cases[0].member = vec![load.clone()];
        let map = DofMap::build(&model);
        let mut behavior = Box::new(sepika_element::frame::fiber::FiberBeam::with_plastic_zone(
            &model.elements[0],
            &model,
            50.,
            sepika_element::factory::StrengthBasis::MaterialStrength,
            sepika_core::model::AnalysisKind::Incremental,
        )) as Box<dyn sepika_element::behavior::ElementBehavior>;
        behavior
            .update_state_with_member_loads(
                &LocalVec {
                    data: smallvec::smallvec![0.; 12],
                },
                &[load],
                true,
                &Ctx { model: &model },
            )
            .unwrap();
        let mut recorder = super::step_record::StepRecorder::new(
            &model,
            SeismicDir::X,
            &[3000.],
            DuctilityMethod::default(),
            super::loads::PhysicalPushoverLoads::empty(),
        )
        .unwrap();
        recorder.physical_loads = super::loads::PhysicalPushoverLoads::new(
            &model,
            &[(model.load_cases[0].id, 1.)],
            super::loads::PhysicalPushoverLoads::empty().reference,
        )
        .unwrap();
        recorder.record(0., &model, &map, &[behavior], &[]).unwrap();
        let result = recorder.finish();
        close(result.capacity_curve[0].base_shear, 6000.);
        close(result.capacity_curve[0].story_shear[0], 6000.);
        close(result.confirmed_history[0].cuts[0].forces[0].force_n, 0.);
    }
}

fn fixed_member_response(
    model: &Model,
    source: LoadCase,
) -> super::physical_response::PhysicalStep {
    let map = DofMap::build(model);
    let behaviors: Vec<Box<dyn sepika_element::behavior::ElementBehavior>> = model
        .elements
        .iter()
        .map(|elem| {
            Box::new(sepika_element::frame::beam::BeamElement::new(elem, model))
                as Box<dyn sepika_element::behavior::ElementBehavior>
        })
        .collect();
    let responses = super::geom::member_recording_responses(model, &behaviors).unwrap();
    super::physical_response::PhysicalStep::recover(
        model,
        &map,
        &behaviors,
        &responses,
        &[],
        source.clone(),
        source,
    )
    .unwrap()
}

#[test]
fn member_endpoints_equal_nodal_loads_but_interior_loads_keep_member_response() {
    for reversed in [false, true] {
        for sign in [-1., 1.] {
            let mut model = super::tests::single_column_model(1e8, 0.);
            model.nodes[1].restraint = Dof6Mask::FIXED;
            if reversed {
                model.elements[0].nodes.reverse();
            }
            let map = DofMap::build(&model);
            for a in [0., 1e-7, 3000. - 1e-7, 3000.] {
                let mut source = super::loads::PhysicalPushoverLoads::empty().reference;
                source.member.push(MemberLoad::manual(
                    model.elements[0].id,
                    [1., 0., 0.],
                    MemberLoadKind::Point { a, p: sign * 1000. },
                ));
                let physical = fixed_member_response(&model, source);
                let forces = physical
                    .capacity_forces(&model, &map, SeismicDir::X)
                    .unwrap();
                if a == 0. || a == 3000. {
                    close(forces.base, 0.);
                    close(forces.story[0], 0.);
                    let mut nodal = super::loads::PhysicalPushoverLoads::empty().reference;
                    nodal.nodal.push(NodalLoad::manual(
                        model.elements[0].nodes[usize::from(a > 0.)],
                        [sign * 1000., 0., 0., 0., 0., 0.],
                    ));
                    let nodal_physical = fixed_member_response(&model, nodal);
                    let nodal_forces = nodal_physical
                        .capacity_forces(&model, &map, SeismicDir::X)
                        .unwrap();
                    close(nodal_forces.base, forces.base);
                    for node in 0..2 {
                        close(nodal_physical.support[node][0], physical.support[node][0]);
                    }
                } else {
                    close(forces.base, sign * 1000.);
                    close(forces.story[0], sign * 1000.);
                }
                close(physical.support.iter().map(|n| n[0]).sum(), -sign * 1000.);
            }
        }
    }
}

#[test]
fn story_member_loads_use_sl_intervals_and_preserve_base_outside_story_membership() {
    for z in [-1., 0., 3000., 6000., 6001.] {
        let mut model = super::tests::single_column_model(1e8, 0.);
        model.nodes[1].restraint = Dof6Mask::FIXED;
        model.nodes[0].coord = [0., 0., z];
        model.nodes[1].coord = [3000., 0., z];
        model.elements[0].local_axis.ref_vector = [0., 0., 1.];
        let mut roof = model.stories[1].clone();
        roof.id = StoryId(2);
        roof.elevation = Some(6000.);
        roof.node_ids.clear();
        model.stories.push(roof);
        for story in &mut model.stories {
            story.elevation = story.elevation.map(|elevation| elevation - 100.);
            story.sl_offset_mm = Some(100.);
        }
        let mut source = super::loads::PhysicalPushoverLoads::empty().reference;
        source.member.push(MemberLoad::manual(
            model.elements[0].id,
            [1., 0., 0.],
            MemberLoadKind::Distributed {
                a: 0.,
                b: 3000.,
                w1: 2.,
                w2: 2.,
            },
        ));
        let physical = fixed_member_response(&model, source);
        let capacity = physical
            .capacity_forces(&model, &DofMap::build(&model), SeismicDir::X)
            .unwrap();
        close(capacity.base, 6000.);
        close(
            capacity.story[0],
            if z > 0. && z <= 6000. { 6000. } else { 0. },
        );
        close(
            capacity.story[1],
            if z > 3000. && z <= 6000. { 6000. } else { 0. },
        );
        model.stories.clear();
        let capacity = physical
            .capacity_forces(&model, &DofMap::build(&model), SeismicDir::X)
            .unwrap();
        close(capacity.base, 6000.);
        assert!(capacity.story.is_empty());
    }
}

#[test]
fn concentrated_load_at_midcut_preserves_material_i_side_limit_under_end_reversal() {
    for reversed in [false, true] {
        for a in [1499., 1500., 1501.] {
            let mut model = super::tests::single_column_model(1e8, 0.);
            model.nodes[1].restraint = Dof6Mask::FIXED;
            if reversed {
                model.elements[0].nodes.reverse();
            }
            let mut source = super::loads::PhysicalPushoverLoads::empty().reference;
            source.member.push(MemberLoad::manual(
                model.elements[0].id,
                [1., 0., 0.],
                MemberLoadKind::Point { a, p: 1000. },
            ));
            let physical = fixed_member_response(&model, source.clone());
            let actual_load =
                super::physical_response::load_above(&model, &source, 1500., 0).unwrap();
            close(
                actual_load,
                if (!reversed && a >= 1500.) || (reversed && a < 1500.) {
                    1000.
                } else {
                    0.
                },
            );
            let bottom = 0;
            let top = 1;
            close(
                physical.support[bottom][0] + physical.support[top][0],
                -1000.,
            );
            close(
                -physical.support[bottom][0],
                if reversed {
                    1000. * (a / 3000.).powi(2) * (3. - 2. * a / 3000.)
                } else {
                    1000. * (1. - (a / 3000.).powi(2) * (3. - 2. * a / 3000.))
                },
            );
            for fiber in [false, true] {
                let map = DofMap::build(&model);
                let mut behavior: Box<dyn sepika_element::behavior::ElementBehavior> = if fiber {
                    Box::new(sepika_element::frame::fiber::FiberBeam::with_plastic_zone(
                        &model.elements[0],
                        &model,
                        50.,
                        sepika_element::factory::StrengthBasis::MaterialStrength,
                        sepika_core::model::AnalysisKind::Incremental,
                    ))
                } else {
                    Box::new(sepika_element::frame::beam::BeamElement::new(
                        &model.elements[0],
                        &model,
                    ))
                };
                if fiber {
                    behavior
                        .update_state_with_member_loads(
                            &LocalVec {
                                data: smallvec::smallvec![0.; 12],
                            },
                            &source.member,
                            true,
                            &Ctx { model: &model },
                        )
                        .unwrap();
                }
                let behaviors = vec![behavior];
                let responses =
                    super::geom::member_recording_responses(&model, &behaviors).unwrap();
                let state = super::physical_response::PhysicalStep::recover(
                    &model,
                    &map,
                    &behaviors,
                    &responses,
                    &[],
                    source.clone(),
                    source.clone(),
                )
                .unwrap();
                let cut = super::story_response::record_cuts_with_responses(
                    &model,
                    &map,
                    &behaviors,
                    SeismicDir::X,
                    &responses,
                    &state,
                )
                .unwrap()
                .remove(0);
                assert!(cut.unavailable.is_none());
                close(cut.forces[0].force_n, actual_load + state.support[1][0]);
            }
        }
    }
}

#[test]
fn partial_tapered_load_uses_exact_resultant_and_upper_cut_load() {
    let mut model = super::tests::single_column_model(1e8, 0.);
    model.nodes[1].restraint = Dof6Mask::FIXED;
    let mut source = super::loads::PhysicalPushoverLoads::empty().reference;
    source.member.push(MemberLoad::manual(
        model.elements[0].id,
        [1., 0., 0.],
        MemberLoadKind::Distributed {
            a: 500.,
            b: 2500.,
            w1: 1.,
            w2: 3.,
        },
    ));
    let physical = fixed_member_response(&model, source.clone());
    let capacity = physical
        .capacity_forces(&model, &DofMap::build(&model), SeismicDir::X)
        .unwrap();
    close(capacity.base, 4000.);
    close(capacity.story[0], 4000.);
    close(
        super::physical_response::load_above(&model, &source, 1500., 0).unwrap(),
        2500.,
    );
    close(physical.support[0][0], -45800. / 27.);
    close(physical.support[1][0], -62200. / 27.);
}

#[test]
fn invalid_physical_load_keeps_all_previous_confirmed_records() {
    let model = super::tests::single_column_model(1e8, 0.);
    let map = DofMap::build(&model);
    let behaviors = vec![sepika_element::factory::build_behavior(
        &model.elements[0],
        &model,
    )];
    let mut recorder = super::step_record::StepRecorder::new(
        &model,
        SeismicDir::X,
        &[3000.],
        DuctilityMethod::default(),
        super::loads::PhysicalPushoverLoads::empty(),
    )
    .unwrap();
    let displacement = vec![0.; map.n_active()];
    recorder
        .record(0., &model, &map, &behaviors, &displacement)
        .unwrap();
    recorder
        .physical_loads
        .reference
        .member
        .push(MemberLoad::manual(
            model.elements[0].id,
            [1., 0., 0.],
            MemberLoadKind::Point {
                a: 1000.,
                p: f64::NAN,
            },
        ));
    assert!(recorder
        .record(1., &model, &map, &behaviors, &displacement)
        .err()
        .unwrap()
        .contains("非有限"));
    let recorded = recorder.finish();
    assert_eq!(recorded.capacity_curve.len(), 1);
    assert_eq!(recorded.steps.len(), 1);
    assert_eq!(recorded.member_history_steps.len(), 1);
    assert_eq!(recorded.confirmed_history.len(), 1);
    assert!(recorded.wall_history.is_empty());
    assert!(recorded.hinges.is_empty());
    assert!(recorded.shear_yields.is_empty());
}

#[test]
fn rigid_link_to_fixed_master_keeps_its_dependent_support_resistance() {
    let mut model = super::tests::single_column_model(1e8, 0.);
    model.nodes[1].restraint = Dof6Mask(0b111110);
    model.constraints.push(Constraint::RigidLink {
        master: NodeId(0),
        slaves: vec![NodeId(1)],
        dofs: Dof6Mask(1),
    });
    let map = DofMap::build(&model);
    assert_eq!(map.n_active(), 1);
    assert_eq!(Reducer::build(&model, &map).n_indep, 0);
    let mut source = super::loads::PhysicalPushoverLoads::empty().reference;
    source.member.push(MemberLoad::manual(
        model.elements[0].id,
        [1., 0., 0.],
        MemberLoadKind::Distributed {
            a: 0.,
            b: 3000.,
            w1: 2.,
            w2: 2.,
        },
    ));
    let physical = fixed_member_response(&model, source);
    let capacity = physical
        .capacity_forces(&model, &map, SeismicDir::X)
        .unwrap();
    close(physical.support[0][0], -3000.);
    close(physical.support[1][0], -3000.);
    close(capacity.base, 6000.);
    close(capacity.story[0], 6000.);
    for as_member in [false, true] {
        let mut direct = super::loads::PhysicalPushoverLoads::empty().reference;
        if as_member {
            direct.member.push(MemberLoad::manual(
                model.elements[0].id,
                [1., 0., 0.],
                MemberLoadKind::Point { a: 3000., p: 1000. },
            ));
        } else {
            direct
                .nodal
                .push(NodalLoad::manual(NodeId(1), [1000., 0., 0., 0., 0., 0.]));
        }
        let physical = fixed_member_response(&model, direct);
        let capacity = physical
            .capacity_forces(&model, &map, SeismicDir::X)
            .unwrap();
        close(physical.support[1][0], -1000.);
        close(capacity.base, 0.);
        close(capacity.story[0], 0.);
    }
}
