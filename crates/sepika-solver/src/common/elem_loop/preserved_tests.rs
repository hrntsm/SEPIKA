use super::*;
fn isolator_and_fiber_model(kind: sepika_core::model::ElementKind, friction: bool) -> Model {
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
    use sepika_core::model::{
        ElementData, ElementKind, EndCondition, ForceRegime, IsolatorAttr, IsolatorKind,
        IsolatorProps, LocalAxis, Material, MaterialCategory, Node, Section,
    };
    let nodes = [
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1000.0],
        [2000.0, 0.0, 0.0],
        [5000.0, 0.0, 0.0],
    ]
    .into_iter()
    .enumerate()
    .map(|(i, coord)| Node {
        id: NodeId(i as u32),
        coord,
        restraint: if i % 2 == 0 {
            Dof6Mask::FIXED
        } else {
            Dof6Mask::FREE
        },
        mass: None,
        story: None,
        support_spring: None,
    })
    .collect();
    let element = |id, kind, nodes, section, reference| ElementData {
        id: ElemId(id),
        kind,
        nodes,
        section,
        local_axis: LocalAxis {
            ref_vector: reference,
        },
        end_cond: [EndCondition::Fixed; 2],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: Some(300.0),
        spring: None,
    };
    let section = Section {
        material: Some(MaterialId(0)),
        area: 20000.0,
        iy: 66666666.66666667,
        iz: 16666666.66666667,
        j: 1e6,
        depth: 200.0,
        width: 100.0,
        ..Section::zero(SectionId(0), "batch Fiber/MS".into())
    };
    Model {
        nodes,
        elements: vec![
            element(
                0,
                ElementKind::Isolator,
                smallvec::smallvec![NodeId(0), NodeId(1)],
                None,
                [1.0, 0.0, 0.0],
            ),
            element(
                1,
                kind,
                smallvec::smallvec![NodeId(2), NodeId(3)],
                Some(SectionId(0)),
                [0.0, 1.0, 0.0],
            ),
        ],
        sections: vec![section],
        materials: vec![Material {
            id: MaterialId(0),
            name: "steel".into(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: Some(78846.15),
            fc: None,
            fy: Some(235.0),
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        isolator_attrs: vec![IsolatorAttr {
            elem: ElemId(0),
            props: IsolatorProps {
                kind: if friction {
                    IsolatorKind::ElasticSliding
                } else {
                    IsolatorKind::HighDampingRubber
                },
                k1: 10.0,
                k2: 1.0,
                qd: 100.0,
                kv: 1000.0,
                mu: 0.1,
                n_long: 1000.0,
                total_rubber_thickness: 100.0,
                ckd_gamma: [1.0, -0.2, 0.0],
                cqd_gamma: [1.0, -0.1, 0.0],
                ..Default::default()
            },
        }],
        ..Default::default()
    }
}

fn real_batch(model: &Model) -> Vec<Box<dyn ElementBehavior>> {
    use sepika_element::factory::{build_nonlinear_behavior, StrengthBasis};
    model
        .elements
        .iter()
        .map(|e| {
            build_nonlinear_behavior(
                e,
                model,
                StrengthBasis::Nominal,
                sepika_core::model::AnalysisKind::Incremental,
                None,
            )
        })
        .collect()
}

#[test]
fn real_isolator_and_failed_fiber_ms_batch_preserves_uncommitted_state_in_both_modes() {
    use sepika_core::model::ElementKind;
    for parallel in [false, true] {
        for friction in [false, true] {
            for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
                for trial_displacement in [1.0, 20.0] {
                    let model = isolator_and_fiber_model(kind, friction);
                    let dofmap = DofMap::build(&model);
                    let ctx = Ctx { model: &model };
                    let mut behaviors = real_batch(&model);
                    let mut du = LocalVec {
                        data: smallvec::smallvec![0.0;12],
                    };
                    du.data[6] = 1.0;
                    behaviors[0].update_state(&du, true, &ctx).unwrap();
                    du.data[6] = trial_displacement - 1.0;
                    behaviors[0].update_state(&du, false, &ctx).unwrap();
                    let checkpoints: Vec<_> =
                        behaviors.iter().map(|b| b.serialize_checkpoint()).collect();
                    let forces: Vec<_> = behaviors
                        .iter()
                        .map(|b| b.internal_force(&ctx).data)
                        .collect();
                    let tangents: Vec<_> = behaviors
                        .iter()
                        .map(|b| b.tangent_stiffness(&ctx).data)
                        .collect();
                    let mut bad = vec![0.0; dofmap.n_active()];
                    bad[dofmap.active(6).unwrap() as usize] = 30.0;
                    bad[dofmap.active(3 * 6 + 3).unwrap() as usize] = f64::MAX;
                    let error =
                        apply_du_trial_mode(&model, &dofmap, &mut behaviors, &bad, parallel)
                            .unwrap_err();
                    assert!(
                        error.contains("ElemId(1)") && error.contains("構成則"),
                        "{error}"
                    );
                    for (i, b) in behaviors.iter().enumerate() {
                        assert_eq!(b.serialize_checkpoint(), checkpoints[i]);
                        assert_eq!(b.internal_force(&ctx).data, forces[i]);
                        assert_eq!(b.tangent_stiffness(&ctx).data, tangents[i]);
                    }
                    let mut control = real_batch(&model);
                    for (b, cp) in control.iter_mut().zip(&checkpoints) {
                        b.deserialize_checkpoint(cp).unwrap();
                    }
                    for batch in [&mut behaviors, &mut control] {
                        for b in batch.iter_mut() {
                            b.commit_state();
                        }
                        let committed: Vec<_> =
                            batch.iter().map(|b| b.serialize_checkpoint()).collect();
                        let committed_forces: Vec<_> =
                            batch.iter().map(|b| b.internal_force(&ctx).data).collect();
                        let committed_tangents: Vec<_> = batch
                            .iter()
                            .map(|b| b.tangent_stiffness(&ctx).data)
                            .collect();
                        let mut good = vec![0.0; dofmap.n_active()];
                        good[dofmap.active(6).unwrap() as usize] = 5.0;
                        apply_du_trial_mode(&model, &dofmap, batch, &good, parallel).unwrap();
                        for (i, b) in batch.iter_mut().enumerate() {
                            b.revert_state();
                            assert_eq!(b.serialize_checkpoint(), committed[i]);
                            assert_eq!(b.internal_force(&ctx).data, committed_forces[i]);
                            assert_eq!(b.tangent_stiffness(&ctx).data, committed_tangents[i]);
                        }
                        apply_du_trial_mode(&model, &dofmap, batch, &good, parallel).unwrap();
                    }
                    for (b, c) in behaviors.iter().zip(&control) {
                        assert_eq!(b.serialize_checkpoint(), c.serialize_checkpoint());
                        assert_eq!(b.internal_force(&ctx).data, c.internal_force(&ctx).data);
                        assert_eq!(
                            b.tangent_stiffness(&ctx).data,
                            c.tangent_stiffness(&ctx).data
                        );
                    }
                }
            }
        }
    }
}
