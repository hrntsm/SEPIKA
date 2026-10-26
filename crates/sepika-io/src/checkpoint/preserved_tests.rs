use super::*;
use sepika_core::model::MaterialCategory;
fn make_checkpoint(model_hash: &str, step: u64) -> Checkpoint {
    Checkpoint {
        schema_version: CHECKPOINT_SCHEMA_VERSION,
        model_hash: model_hash.into(),
        step,
        time: 0.0,
        disp: vec![],
        vel: vec![],
        accel: vec![],
        state: StateBlob {
            element_states: vec![],
        },
    }
}
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Material, Model, Node, Section,
};
use sepika_element::behavior::{Ctx, ElementBehavior, LocalVec};
use sepika_element::factory::{build_nonlinear_behavior, AnalysisKind, StrengthBasis};
use smallvec::smallvec;

fn fiber_model(young: f64) -> Model {
    Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [1000.0, 0.0, 0.0],
                restraint: Dof6Mask(0b111110),
                mass: Some([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
                story: None,
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(1),
            kind: ElementKind::Fiber,
            nodes: smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: Some(100.0),
            spring: None,
        }],
        sections: vec![Section {
            frame_use: None,
            id: SectionId(0),
            name: "spring".into(),
            area: 1.0,
            iy: 1.0,
            iz: 1.0,
            j: 1.0,
            depth: 1.0,
            width: 1.0,
            as_y: 1e12,
            as_z: 1e12,
            floor: None,
            panel_thickness: None,
            thickness: None,
            shape: None,
            material: Some(MaterialId(0)),
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: Default::default(),
        }],
        materials: vec![Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(0),
            name: "mat".into(),
            category: MaterialCategory::Steel,
            young,
            poisson: 0.0,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(200.0),
        }],
        ..Default::default()
    }
}

fn build_behaviors(model: &Model, count: usize) -> Vec<Box<dyn ElementBehavior>> {
    (0..count)
        .map(|_| {
            build_nonlinear_behavior(
                &model.elements[0],
                model,
                StrengthBasis::Nominal,
                AnalysisKind::Incremental,
                None,
            )
        })
        .collect()
}

#[derive(Debug, PartialEq)]
struct Observation {
    checkpoint: Vec<u8>,
    force: Vec<f64>,
    tangent: Vec<f64>,
    fibers: Vec<u8>,
}

fn observations(behaviors: &[Box<dyn ElementBehavior>], ctx: &Ctx) -> Vec<Observation> {
    behaviors
        .iter()
        .map(|b| Observation {
            checkpoint: b.serialize_checkpoint(),
            force: b.internal_force(ctx).data.to_vec(),
            tangent: b.tangent_stiffness(ctx).data.to_vec(),
            fibers: bincode::serialize(&b.fiber_section_states()).unwrap(),
        })
        .collect()
}

fn increment(behaviors: &mut [Box<dyn ElementBehavior>], delta: f64, commit: bool, ctx: &Ctx) {
    let mut du = LocalVec {
        data: smallvec![0.0; 12],
    };
    du.data[6] = delta;
    for b in behaviors {
        b.update_state(&du, commit, ctx).unwrap();
    }
}

fn prepare_trial(behaviors: &mut [Box<dyn ElementBehavior>], ctx: &Ctx) {
    increment(behaviors, 2.0, true, ctx);
    increment(behaviors, -0.3, false, ctx);
}

fn assert_failed_load_preserves_trial_and_commit(
    dir: &Path,
    model: &Model,
    cp: &Checkpoint,
    behaviors: &mut [Box<dyn ElementBehavior>],
) {
    let ctx = Ctx { model };
    let before = observations(behaviors, &ctx);
    let snapshots: Vec<_> = behaviors.iter().map(|b| b.snapshot_state()).collect();
    for b in behaviors.iter_mut() {
        b.revert_state();
    }
    let committed = observations(behaviors, &ctx);
    assert_ne!(before, committed);
    for (b, snapshot) in behaviors.iter_mut().zip(&snapshots) {
        b.restore_state(snapshot.as_ref());
    }
    save_checkpoint(dir, cp).unwrap();
    let Err(error) = load_nonlinear_checkpoint(dir, model, behaviors) else {
        panic!("不正なチェックポイントを拒否する");
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(observations(behaviors, &ctx), before);
    for b in behaviors.iter_mut() {
        b.revert_state();
    }
    assert_eq!(observations(behaviors, &ctx), committed);
    for (b, snapshot) in behaviors.iter_mut().zip(&snapshots) {
        b.restore_state(snapshot.as_ref());
    }
}

#[test]
fn nonlinear_checkpoint_count_mismatch_and_second_decode_failure_preserve_all_elements() {
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        let mut model = fiber_model(200000.0);
        model.elements[0].kind = kind;
        let ctx = Ctx { model: &model };
        let mut behaviors = build_behaviors(&model, 2);
        prepare_trial(&mut behaviors, &ctx);
        let mut source = build_behaviors(&model, 2);
        increment(&mut source, 0.5, true, &ctx);
        let states: Vec<_> = source.iter().map(|b| b.serialize_checkpoint()).collect();
        let dir = crate::test_util::test_tmp().join(format!("cp_atomic_{kind:?}"));
        for count in [1, 3] {
            let mut cp = make_checkpoint(&compute_model_hash(&model), 2);
            cp.state.element_states = vec![states[0].clone(); count];
            assert_failed_load_preserves_trial_and_commit(&dir, &model, &cp, &mut behaviors);
        }
        let mut cp = make_checkpoint(&compute_model_hash(&model), 2);
        cp.state.element_states = vec![states[0].clone(), vec![0xff]];
        assert_failed_load_preserves_trial_and_commit(&dir, &model, &cp, &mut behaviors);
        cp.model_hash = "different".into();
        cp.state.element_states = states;
        assert_failed_load_preserves_trial_and_commit(&dir, &model, &cp, &mut behaviors);
    }
}

#[test]
fn fiber_and_multi_spring_checkpoint_resume_matches_uninterrupted_axial_yield_unload_reload() {
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        let mut model = fiber_model(200000.0);
        model.elements[0].kind = kind;
        let ctx = Ctx { model: &model };
        let mut uninterrupted = build_behaviors(&model, 1);
        prepare_trial(&mut uninterrupted, &ctx);
        let peak = uninterrupted[0].fiber_section_states().unwrap();
        assert!(peak
            .iter()
            .flat_map(|s| &s.fibers)
            .any(|f| f.yield_ratio > 1.0));
        let force = uninterrupted[0].internal_force(&ctx).data[6];
        assert!(force > 0.0 && force < 200000.0 / 1000.0 * 1.7);
        let dir = crate::test_util::test_tmp().join(format!("cp_resume_{kind:?}"));
        save_nonlinear_checkpoint(
            &dir,
            &model,
            42,
            0.42,
            &[1.7],
            &[0.1],
            &[0.0],
            &uninterrupted,
        )
        .unwrap();
        let mut restored = build_behaviors(&model, 1);
        let cp = load_nonlinear_checkpoint(&dir, &model, &mut restored).unwrap();
        assert_eq!(cp.schema_version, CHECKPOINT_SCHEMA_VERSION);
        assert_eq!(cp.step, 42);
        assert_eq!(cp.time, 0.42);
        assert_eq!(cp.disp, vec![1.7]);
        assert_eq!(cp.vel, vec![0.1]);
        assert_eq!(cp.accel, vec![0.0]);
        assert_eq!(
            observations(&restored, &ctx),
            observations(&uninterrupted, &ctx)
        );
        for b in [&mut restored[0], &mut uninterrupted[0]] {
            b.revert_state();
        }
        assert_eq!(
            observations(&restored, &ctx),
            observations(&uninterrupted, &ctx)
        );
        for delta in [-0.5, -1.5, -1.0, 1.0, 2.5] {
            increment(&mut uninterrupted, delta, false, &ctx);
            increment(&mut restored, delta, false, &ctx);
            assert_eq!(
                observations(&restored, &ctx),
                observations(&uninterrupted, &ctx)
            );
            for b in [&mut restored[0], &mut uninterrupted[0]] {
                b.commit_state();
            }
        }
    }
}

#[test]
fn previous_and_future_checkpoint_versions_leave_all_states_unchanged() {
    let model = fiber_model(200000.0);
    let ctx = Ctx { model: &model };
    let mut behaviors = build_behaviors(&model, 2);
    prepare_trial(&mut behaviors, &ctx);
    let dir = crate::test_util::test_tmp().join("cp_preserved_versions");
    for version in [1, 2, 3, CHECKPOINT_SCHEMA_VERSION + 1] {
        let mut cp = make_checkpoint(&compute_model_hash(&model), 2);
        cp.schema_version = version;
        cp.state.element_states = behaviors.iter().map(|b| b.serialize_checkpoint()).collect();
        assert_failed_load_preserves_trial_and_commit(&dir, &model, &cp, &mut behaviors);
    }
}
