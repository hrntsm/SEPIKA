use super::*;
use approx::assert_relative_eq;
use sepika_core::ids::ElemId;
use sepika_core::model::{MemberLoadKind, Model};
use sepika_material::uniaxial::Bilinear;

fn member(hardening: f64) -> FiberBeam {
    member_length(hardening, 1000.0)
}

fn member_length(hardening: f64, length: f64) -> FiberBeam {
    member_properties(hardening, length, 0.0, 0.0)
}
fn member_properties(hardening: f64, length: f64, g: f64, shear_area: f64) -> FiberBeam {
    let sections = std::array::from_fn(|_| {
        let fibers = [(-10.0, -20.0), (10.0, -20.0), (-10.0, 20.0), (10.0, 20.0)]
            .into_iter()
            .map(|(y, z)| Fiber {
                y,
                z,
                area: 250.0,
                material: 2,
            })
            .collect();
        let mats = (0..4)
            .map(|_| {
                Box::new(Bilinear::new(200000.0, 200.0, hardening)) as Box<dyn UniaxialMaterial>
            })
            .collect();
        (FiberSection { fibers }, mats)
    });
    let mut result = FiberBeam::from_raw_parts(
        [NodeId(0), NodeId(1)],
        length,
        crate::transform::LocalFrame::from_nodes([0.0; 3], [1000.0, 0.0, 0.0], [0.0, 1.0, 0.0])
            .unwrap(),
        SectionMassProperties::default(),
        200000.0,
        g,
        1000.0,
        400000.0,
        100000.0,
        shear_area,
        shear_area,
        0.0,
        100.0,
        sections,
    );
    result.element_id = Some(ElemId(7));
    result
}

fn udl(w: f64, axis: usize) -> MemberLoad {
    let mut dir = [0.0; 3];
    dir[axis] = 1.0;
    MemberLoad::manual(
        ElemId(7),
        dir,
        MemberLoadKind::Distributed {
            a: 0.0,
            b: 1000.0,
            w1: w,
            w2: w,
        },
    )
}
fn zero() -> LocalVec {
    LocalVec {
        data: smallvec::smallvec![0.0;12],
    }
}
fn apply(m: &mut FiberBeam, w: f64, axis: usize, commit: bool) {
    let model = Model::default();
    m.update_state_with_member_loads(&zero(), &[udl(w, axis)], commit, &Ctx { model: &model })
        .unwrap();
}
fn raw_force(m: &FiberBeam) -> [f64; 12] {
    FiberBeam::hinge_internal_force(m.hinge.as_ref().unwrap(), &m.elem_disp(&m.flex_disp()))
}
fn q(m: &FiberBeam, end: usize) -> [f64; 3] {
    m.gauss_points[end].cached_force
}

fn connect(
    m: &mut FiberBeam,
    condition: sepika_core::model::EndCondition,
    normal: Option<[f64; 3]>,
) {
    m.releases = resolve_end_releases(&[condition; 2], [false; 2], false);
    m.trial_int = SmallVec::from_elem(0.0, m.releases.len());
    m.committed_int = m.trial_int.clone();
    if let Some(normal) = normal {
        m.configure_wall_release(normal).unwrap();
    }
}
fn close_force(actual: &[f64], expected: &[f64]) {
    for (a, b) in actual.iter().zip(expected) {
        assert_relative_eq!(a, b, epsilon = 1e-6, max_relative = 2e-8);
    }
}
fn rigid_member() -> FiberBeam {
    let mut m = member_length(0.01, 700.0);
    set_rigid_geometry(&mut m);
    m
}
fn set_rigid_geometry(m: &mut FiberBeam) {
    m.length = 1000.;
    m.rigid_i = 200.;
    m.rigid_j = 100.;
    for (i, j, sign) in [(0, 0, 1.), (6, 6, 1.), (0, 6, -1.), (6, 0, -1.)] {
        m.initial_elastic_stiffness.set(i, j, sign * 200000.);
        m.hinge.as_mut().unwrap().k_el.set(i, j, sign * 200000.);
    }
}
fn point(a: f64) -> MemberLoad {
    MemberLoad::manual(
        ElemId(7),
        [0., 1., 0.],
        MemberLoadKind::Point { a, p: 1000.0 },
    )
}

#[test]
fn rigid_point_bypasses_material_and_keeps_physical_force_arm_for_original_and_wall_connections() {
    use sepika_core::model::EndCondition::*;
    for condition in [Fixed, Pinned, SemiRigid { k_theta: 1e7 }] {
        for normal in [None, Some([0., 0., 1.]), Some([0., 0.6, 0.8])] {
            let mut m = rigid_member();
            connect(&mut m, condition, normal);
            m.try_update_state_with_member_loads(&zero(), Some(&[point(100.)]), false)
                .unwrap();
            close_force(&m.flex_member_load(), &[0.; 12]);
            assert!(m
                .gauss_points
                .iter()
                .all(|gp| gp.trial_stress.iter().all(|s| s.abs() < 1e-10)));
            let mut expected = [0.; 12];
            expected[1] = -1000.;
            expected[5] = -100000.;
            let response = m.current_loaded_response().unwrap().unwrap();
            close_force(&response.physical_frame_end_force_global, &expected);
            close_force(&response.nodal_residual.data, &expected);
            assert!(m.trial_int.iter().all(|v| v.abs() < 1e-12));
        }
    }
}

#[test]
fn flex_point_wall_equilibrium_keeps_nonzero_raw_moments_and_zero_released_face_moments() {
    let mut m = rigid_member();
    connect(
        &mut m,
        sepika_core::model::EndCondition::Fixed,
        Some([0., 0., 1.]),
    );
    m.try_update_state_with_member_loads(&zero(), Some(&[point(550.)]), false)
        .unwrap();
    let raw = raw_force(&m);
    assert_relative_eq!(raw[5], 87500., epsilon = 1e-6);
    assert_relative_eq!(raw[11], -87500., epsilon = 1e-6);
    let response = m.current_loaded_response().unwrap().unwrap();
    assert!(response.flex_face_force_local[5].abs() < 1e-6);
    assert!(response.flex_face_force_local[11].abs() < 1e-6);
    let mut expected = [0.; 12];
    expected[1] = -500.;
    expected[7] = -500.;
    expected[5] = -100000.;
    expected[11] = 50000.;
    close_force(&response.physical_frame_end_force_global, &expected);
    close_force(&response.nodal_residual.data, &expected);
    let model = Model::default();
    let ctx = Ctx { model: &model };
    let recovered = m.state_member_forces(&ctx).unwrap();
    let direct = response.section.recover(&m.eval_sections).unwrap();
    assert_eq!(format!("{recovered:?}"), format!("{direct:?}"));
}

fn equivalent_beam(
    m: &FiberBeam,
    condition: sepika_core::model::EndCondition,
) -> crate::frame::beam::BeamElement {
    crate::frame::beam::BeamElement {
        id: ElemId(7),
        e: 200000.,
        g: 80000.,
        a: 1000.,
        iy: 400000.,
        iz: 100000.,
        j: 0.,
        as_y: 1000.,
        as_z: 1000.,
        length: m.length,
        mass_properties: SectionMassProperties::default(),
        mass_properties_error: None,
        nodes: m.nodes,
        axis: m.axis,
        rigid: sepika_core::model::RigidZone {
            length_i: m.rigid_i,
            length_j: m.rigid_j,
            ..Default::default()
        },
        end_cond: [condition; 2],
        torsion_release: [false; 2],
        eval_sections: m.eval_sections.clone(),
        section: None,
        material: None,
        committed_disp: [0.; 12],
        trial_disp: [0.; 12],
        committed_member_loads: Vec::new(),
        trial_member_loads: Vec::new(),
        local_stiffness_cache: std::sync::OnceLock::new(),
    }
}

#[test]
fn elastic_original_and_oblique_wall_loaded_states_match_linear_beam_forces_and_work() {
    use sepika_core::model::EndCondition::*;
    for rigid in [false, true] {
        for condition in [Fixed, Pinned, SemiRigid { k_theta: 1e7 }] {
            for normal in [
                None,
                Some([0., 0., 1.]),
                Some([0., 0.6, 0.8]),
                Some([0., -0.6, -0.8]),
            ] {
                let mut m =
                    member_properties(0.01, if rigid { 700.0 } else { 1000.0 }, 80000.0, 1000.0);
                if rigid {
                    set_rigid_geometry(&mut m);
                }
                connect(&mut m, condition, normal);
                let beam = equivalent_beam(&m, condition);
                let loads = [udl(2., 1), udl(1., 2), udl(1., 0)];
                let mut u = zero();
                u.data[6] = 0.002;
                u.data[7] = 0.003;
                u.data[10] = 1e-6;
                let expected = beam
                    .loaded_response(&u.data, &loads, m.wall_connection.clone())
                    .unwrap();
                m.try_update_state_with_member_loads(&u, Some(&loads), false)
                    .unwrap();
                let response = m.current_loaded_response().unwrap().unwrap();
                close_force(&response.nodal_residual.data, &expected.nodal_residual.data);
                close_force(
                    &response.physical_frame_end_force_global,
                    &expected.physical_frame_end_force_global,
                );
                close_force(
                    &response.flex_face_force_local,
                    &expected.flex_face_force_local,
                );
                for xi in [0., 0.3, 0.5, 0.7, 1.] {
                    close_force(
                        &response.section.at(xi).unwrap(),
                        &expected.section.at(xi).unwrap(),
                    );
                }
                let work: f64 = u
                    .data
                    .iter()
                    .zip(&response.nodal_residual.data)
                    .map(|(u, f)| u * f)
                    .sum();
                let expected_work: f64 = u
                    .data
                    .iter()
                    .zip(&expected.nodal_residual.data)
                    .map(|(u, f)| u * f)
                    .sum();
                assert_relative_eq!(work, expected_work, epsilon = 1e-8);
                assert!(m.owns_frame_member_loads());
            }
        }
    }
}

#[test]
fn loaded_outer_yield_unload_finite_difference_and_rollback_share_one_commit() {
    use sepika_core::model::EndCondition::*;
    let model = Model::default();
    let ctx = Ctx { model: &model };
    for normal in [None, Some([0., 0., 1.]), Some([0., 0.6, 0.8])] {
        let mut m = member(0.01);
        connect(&mut m, SemiRigid { k_theta: 1e7 }, normal);
        apply(&mut m, 500., 0, true);
        for load in [300., 500., 300.] {
            m.revert_state();
            let snapshot = m.snapshot_state();
            let mut u = zero();
            u.data[6] = 0.2;
            u.data[7] = 0.005;
            let loads = [udl(load, 0), udl(2., 1)];
            m.try_update_state_with_member_loads(&u, Some(&loads), false)
                .unwrap();
            let tangent = m.tangent_stiffness(&ctx);
            let before = m.serialize_checkpoint();
            let mut bad = udl(1., 1);
            bad.elem = ElemId(99);
            assert!(m
                .try_update_state_with_member_loads(&u, Some(&[bad]), true)
                .is_err());
            assert_eq!(m.serialize_checkpoint(), before);
            for column in [0, 1, 4, 5, 6, 7, 10, 11] {
                let mut force = Vec::new();
                for sign in [-1., 1.] {
                    m.restore_state(snapshot.as_ref());
                    let mut delta = LocalVec {
                        data: u.data.clone(),
                    };
                    delta.data[column] += sign * 1e-7;
                    m.try_update_state_with_member_loads(&delta, Some(&loads), false)
                        .unwrap();
                    force.push(
                        m.current_loaded_response()
                            .unwrap()
                            .unwrap()
                            .nodal_residual
                            .data,
                    );
                }
                for row in 0..12 {
                    let fd = (force[1][row] - force[0][row]) / 2e-7;
                    assert_relative_eq!(
                        tangent.get(row, column),
                        fd,
                        epsilon = 0.2,
                        max_relative = 2e-5
                    );
                }
            }
            m.restore_state(snapshot.as_ref());
            for factor in [0., 0.5, 1., 0.5] {
                apply(&mut m, load * factor, 0, false);
                assert_relative_eq!(q(&m, 0)[0], load * factor * 500., epsilon = 1e-6);
            }
        }
    }
}

#[test]
fn failed_loaded_outer_material_solve_reverts_rotations_history_and_all_response_state() {
    use sepika_core::model::EndCondition::*;
    for normal in [None, Some([0., 0., 1.]), Some([0., 0.6, 0.8])] {
        let mut m = member(0.0);
        connect(&mut m, SemiRigid { k_theta: 1e7 }, normal);
        m.try_update_state_with_member_loads(&zero(), Some(&[udl(2., 1)]), true)
            .unwrap();
        let snapshot = m.snapshot_state();
        let before = m.serialize_checkpoint();
        let mut du = zero();
        du.data[7] = 0.01;
        let error = m
            .try_update_state_with_member_loads(&du, Some(&[udl(500., 0), udl(3., 1)]), true)
            .unwrap_err();
        assert!(error.to_string().contains("内側"));
        assert_eq!(m.serialize_checkpoint(), before);
        let response = m.current_loaded_response().unwrap().unwrap();
        m.try_update_state_with_member_loads(&du, Some(&[udl(3., 1)]), false)
            .unwrap();
        let retried = m.serialize_checkpoint();
        m.restore_state(snapshot.as_ref());
        close_force(
            &m.current_loaded_response()
                .unwrap()
                .unwrap()
                .physical_frame_end_force_global,
            &response.physical_frame_end_force_global,
        );
        m.try_update_state_with_member_loads(&du, Some(&[udl(3., 1)]), false)
            .unwrap();
        assert_eq!(m.serialize_checkpoint(), retried);
    }
}

#[test]
fn tiny_nonzero_load_after_large_uncommitted_trial_matches_same_committed_history() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    for plastic in [false, true] {
        let mut m = member(0.01);
        if plastic {
            let mut du = zero();
            du.data[6] = 2.;
            m.update_state(&du, true, &ctx).unwrap();
        }
        let committed = m.snapshot_state();
        let mut large = zero();
        large.data[6] = 0.2;
        m.update_state_with_member_loads(&large, &[udl(500., 0)], false, &ctx)
            .unwrap();
        let mut undo = zero();
        undo.data[6] = -0.2;
        let loads = [udl(1e-8, 1)];
        m.update_state_with_member_loads(&undo, &loads, false, &ctx)
            .unwrap();
        let actual = m.state_frame_load_response().unwrap().unwrap();
        let actual_q = [q(&m, 0), q(&m, 1)];
        m.restore_state(committed.as_ref());
        m.update_state_with_member_loads(&zero(), &loads, false, &ctx)
            .unwrap();
        let expected = m.state_frame_load_response().unwrap().unwrap();
        close_force(
            &actual.physical_frame_end_force_global,
            &expected.physical_frame_end_force_global,
        );
        for end in 0..2 {
            close_force(&actual_q[end], &q(&m, end));
        }
        let transverse: f64 =
            actual.physical_frame_end_force_global[1] + actual.physical_frame_end_force_global[7];
        assert_relative_eq!(transverse, -1e-5, epsilon = 1e-12);
    }
}
