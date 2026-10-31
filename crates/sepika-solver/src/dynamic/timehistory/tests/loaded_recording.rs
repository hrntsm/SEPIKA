use super::*;
use crate::common::transaction::StateSnapshot;
use crate::dynamic::timehistory::nonlinear::checked_step_member_forces;
use crate::dynamic::timehistory::recording::member_forces_nonlinear;
use crate::dynamic::timehistory::recording::ThRecorder;
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_element::behavior::ElementBehavior;
use sepika_element::behavior::{Ctx, LoadedFrameResponse, LocalMat, LocalVec};
use sepika_element::factory::{build_nonlinear_behavior, StrengthBasis};
use sepika_element::frame::beam::MemberForces;
use sepika_element::frame::fiber::FiberBeam;
use std::any::Any;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn model(rotated: bool, kind: ElementKind) -> Model {
    let mut model = Model {
        nodes: (0..2)
            .map(|i| Node {
                id: NodeId(i),
                coord: if rotated {
                    [0., 1000. * i as f64, 0.]
                } else {
                    [1000. * i as f64, 0., 0.]
                },
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        materials: vec![Material {
            id: MaterialId(0),
            name: "記録試験".into(),
            category: MaterialCategory::Steel,
            young: 205000.,
            poisson: 0.3,
            density: 0.,
            shear: Some(80000.),
            fc: None,
            fy: Some(1e8),
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        sections: vec![Section {
            material: Some(MaterialId(0)),
            area: 20000.,
            iy: 66666666.66666667,
            iz: 16666666.66666667,
            j: 1e6,
            depth: 200.,
            width: 100.,
            ..Section::zero(SectionId(0), "記録試験".into())
        }],
        ..Default::default()
    };
    model.elements.push(ElementData {
        id: ElemId(10),
        kind,
        nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: if rotated { [-1., 0., 0.] } else { [0., 1., 0.] },
        },
        end_cond: [EndCondition::Fixed; 2],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: Some(100.),
        spring: None,
    });
    model
}

fn member(model: &Model) -> Box<dyn ElementBehavior> {
    build_nonlinear_behavior(
        &model.elements[0],
        model,
        StrengthBasis::Nominal,
        AnalysisKind::TimeHistory,
        None,
    )
}

fn zero() -> LocalVec {
    LocalVec {
        data: smallvec::smallvec![0.; 12],
    }
}

fn udl(model: &Model, factor: f64) -> MemberLoad {
    MemberLoad::manual(
        model.elements[0].id,
        model.elements[0].local_axis.ref_vector,
        MemberLoadKind::Distributed {
            a: 0.,
            b: 1000.,
            w1: 2. * factor,
            w2: 2. * factor,
        },
    )
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-7 * expected.abs().max(1.),
        "{actual} != {expected}"
    );
}

#[test]
fn loaded_recording_keeps_signed_udl_sections_and_unstored_step_peaks() {
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        for rotated in [false, true] {
            let model = model(rotated, kind);
            let map = DofMap::build(&model);
            let mut behaviors = vec![member(&model)];
            let mut recorder = ThRecorder::new(&model, &map, 4, Some(2));
            for (step, factor) in [1., -3., 0., 2., -1.].into_iter().enumerate() {
                behaviors[0]
                    .update_state_with_member_loads(
                        &zero(),
                        &[udl(&model, factor)],
                        true,
                        &Ctx { model: &model },
                    )
                    .unwrap();
                let bytes = behaviors[0].serialize_checkpoint();
                let forces = member_forces_nonlinear(&model, &behaviors).unwrap();
                assert_eq!(bytes, behaviors[0].serialize_checkpoint());
                let sections = &forces[0].as_ref().unwrap().at;
                assert!(sections.len() > 2);
                for &(xi, f) in sections {
                    close(f[1], factor * (-1000. + 2000. * xi));
                    close(f[5], factor * (500000. / 3. - 1e6 * xi + 1e6 * xi * xi));
                    for k in [0, 2, 3, 4] {
                        close(f[k], 0.);
                    }
                }
                recorder.record_step(
                    step as u64,
                    step as f64,
                    &model,
                    &map,
                    &[],
                    &[],
                    &[],
                    &[],
                    &[],
                    &[],
                    0.,
                    0.,
                    &forces,
                );
            }
            let recording = recorder.finish();
            assert_eq!(recording.frame_time, [0., 2., 4.]);
            for (frame, factor) in recording.member_forces.iter().zip([1., 0., -1.]) {
                let f = frame[0].as_ref().unwrap();
                assert_eq!(f.at.len(), 2);
                close(f.at[0].1[5], factor * 500000. / 3.);
                close(f.at[1].1[5], factor * 500000. / 3.);
            }
            let peak = recording.peak_member_forces[0].as_ref().unwrap();
            assert!(peak.at.len() > 2);
            for &(xi, f) in &peak.at {
                close(f[5], -3. * (500000. / 3. - 1e6 * xi + 1e6 * xi * xi));
            }
        }
    }
}

#[test]
fn loaded_recording_point_force_keeps_left_limit_and_both_ends() {
    for rotated in [false, true] {
        let model = model(rotated, ElementKind::Beam);
        let mut beam = FiberBeam::new(
            &model.elements[0],
            &model,
            StrengthBasis::Nominal,
            AnalysisKind::TimeHistory,
        );
        beam.eval_sections = vec![0., 0.5, 0.5001, 1.];
        let point = MemberLoad::manual(
            model.elements[0].id,
            model.elements[0].local_axis.ref_vector,
            MemberLoadKind::Point { a: 500., p: 1000. },
        );
        beam.update_state_with_member_loads(&zero(), &[point], false, &Ctx { model: &model })
            .unwrap();
        let forces = member_forces_nonlinear(&model, &[Box::new(beam)]).unwrap();
        let at = &forces[0].as_ref().unwrap().at;
        assert_eq!(
            at.iter().map(|p| p.0).collect::<Vec<_>>(),
            [0., 0.5, 0.5001, 1.]
        );
        for (i, expected) in [-500., -500., 500., 500.].into_iter().enumerate() {
            close(at[i].1[1], expected);
        }
        close(at[0].1[5], 125000.);
        close(at[1].1[5], -125000.);
        close(at[3].1[5], 125000.);
    }
}

struct Probe {
    inner: FiberBeam,
    failure: usize,
    trial: usize,
    committed: usize,
    old_getters: Arc<AtomicUsize>,
    commits: Arc<AtomicUsize>,
}

impl ElementBehavior for Probe {
    fn n_dof(&self) -> usize {
        12
    }
    fn global_dofs(&self, map: &DofMap) -> smallvec::SmallVec<[usize; 24]> {
        self.inner.global_dofs(map)
    }
    fn tangent_stiffness(&self, ctx: &Ctx) -> LocalMat {
        self.inner.tangent_stiffness(ctx)
    }
    fn internal_force(&self, ctx: &Ctx) -> LocalVec {
        self.inner.internal_force(ctx)
    }
    fn mass_matrix(&self, opt: MassOption) -> LocalMat {
        self.inner.mass_matrix(opt)
    }
    fn owns_frame_member_loads(&self) -> bool {
        self.failure != 12
    }
    fn state_frame_load_response(&self) -> Result<Option<LoadedFrameResponse>, String> {
        let mut response = self.inner.state_frame_load_response()?.unwrap();
        match self.failure {
            1 => return Err("記録試験エラー".into()),
            2 => return Ok(None),
            3 => {
                response.nodal_residual.data.pop();
            }
            4 => response.nodal_residual.data[0] = f64::NAN,
            5 => response.physical_frame_end_force_global[0] = f64::INFINITY,
            6 => response.flex_face_force_local[5] = f64::NAN,
            _ => {}
        }
        Ok(Some(response))
    }
    fn state_member_forces(&self, ctx: &Ctx) -> Option<MemberForces> {
        self.old_getters.fetch_add(1, Ordering::Relaxed);
        assert!(
            self.failure == 0 || self.failure >= 7,
            "不正ownerの旧getterへ到達"
        );
        let mut f = self.inner.state_member_forces(ctx).unwrap();
        match self.failure {
            7 => return None,
            8 => f.at.clear(),
            9 => f.at[0].0 = f64::NAN,
            10 => f.at[0].0 = 1.1,
            11 => f.at[0].1[5] = f64::INFINITY,
            _ => {}
        }
        Some(f)
    }
    fn update_state_with_member_loads(
        &mut self,
        du: &LocalVec,
        loads: &[MemberLoad],
        commit: bool,
        ctx: &Ctx,
    ) -> Result<(), sepika_element::behavior::ElementUpdateError> {
        self.inner
            .update_state_with_member_loads(du, loads, commit, ctx)?;
        self.trial += 1;
        Ok(())
    }
    fn snapshot_state(&self) -> Box<dyn Any> {
        Box::new((self.trial, self.committed, self.inner.snapshot_state()))
    }
    fn restore_state(&mut self, state: &dyn Any) {
        let (trial, committed, inner) = state
            .downcast_ref::<(usize, usize, Box<dyn Any>)>()
            .unwrap();
        self.trial = *trial;
        self.committed = *committed;
        self.inner.restore_state(inner.as_ref());
    }
    fn commit_state(&mut self) {
        self.commits.fetch_add(1, Ordering::Relaxed);
        self.committed = self.trial;
    }
    fn serialize_checkpoint(&self) -> Vec<u8> {
        let mut bytes = self.inner.serialize_checkpoint();
        bytes.extend([self.trial as u8, self.committed as u8]);
        bytes
    }
}

#[test]
fn loaded_recording_rejects_bad_owner_before_legacy_getters_and_restores_step() {
    for failure in 1..=11 {
        let mut model = model(false, ElementKind::Beam);
        let mut second = model.elements[0].clone();
        second.id = ElemId(11);
        model.elements.push(second);
        let old_getters = Arc::new(AtomicUsize::new(0));
        let commits = Arc::new(AtomicUsize::new(0));
        let mut behaviors: Vec<Box<dyn ElementBehavior>> = (0..2)
            .map(|i| {
                Box::new(Probe {
                    inner: FiberBeam::new(
                        &model.elements[i],
                        &model,
                        StrengthBasis::Nominal,
                        AnalysisKind::TimeHistory,
                    ),
                    failure: if i == 1 { failure } else { 0 },
                    trial: 3,
                    committed: 2,
                    old_getters: old_getters.clone(),
                    commits: commits.clone(),
                }) as Box<dyn ElementBehavior>
            })
            .collect();
        let expected: Vec<_> = behaviors.iter().map(|b| b.serialize_checkpoint()).collect();
        let snapshot = StateSnapshot::capture(&behaviors);
        for (b, e) in behaviors.iter_mut().zip(&model.elements) {
            let mut load = udl(&model, 2.);
            load.elem = e.id;
            b.update_state_with_member_loads(&zero(), &[load], false, &Ctx { model: &model })
                .unwrap();
        }
        assert_ne!(behaviors[0].serialize_checkpoint(), expected[0]);
        let error = checked_step_member_forces(&model, &mut behaviors, &snapshot, 4, 0.004)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("要素 ID 11")
                && error.contains("非線形時刻歴")
                && error.contains("step 4"),
            "{error}"
        );
        assert_eq!(commits.load(Ordering::Relaxed), 0);
        if failure <= 6 {
            assert_eq!(old_getters.load(Ordering::Relaxed), 0);
        }
        for (b, before) in behaviors.iter().zip(expected) {
            assert_eq!(b.serialize_checkpoint(), before);
        }
    }
}

#[test]
fn loaded_recording_rejects_count_mismatch_and_preserves_nonowner_sections() {
    let model = model(false, ElementKind::Beam);
    assert!(member_forces_nonlinear(&model, &[]).is_err());
    let b = Probe {
        inner: FiberBeam::new(
            &model.elements[0],
            &model,
            StrengthBasis::Nominal,
            AnalysisKind::TimeHistory,
        ),
        failure: 12,
        trial: 0,
        committed: 0,
        old_getters: Arc::new(AtomicUsize::new(0)),
        commits: Arc::new(AtomicUsize::new(0)),
    };
    let expected = b
        .inner
        .state_member_forces(&Ctx { model: &model })
        .unwrap()
        .at;
    assert_eq!(
        member_forces_nonlinear(&model, &[Box::new(b)]).unwrap()[0]
            .as_ref()
            .unwrap()
            .at,
        expected
    );
}
