use approx::assert_relative_eq;
use sepika_core::model::Model;
use sepika_design_jp::secondary::eccentricity_analysis::{
    column_horizontal_stiffnesses, story_eccentricity_from_analysis,
};
use sepika_design_jp::secondary::holding_capacity::{eccentricity_ratio, fes, stiffness_ratios};
use sepika_design_jp::secondary::stiffness_ratio::cog_story_drifts;
use sepika_solver::statics::analysis::SeismicDir;
use sepika_solver::statics::linear::{linear_static_once, StaticOnce};

#[allow(dead_code)]
#[path = "fixtures/eccentricity_model.rs"]
mod eccentricity_model;
use eccentricity_model::{element_nodal_residual, TOP};

#[path = "fixtures/multi_floor_torsional_cancellation_model.rs"]
mod multi_floor_model;
#[path = "fixtures/multi_floor_torsional_cancellation_reference.rs"]
mod multi_floor_reference;
use multi_floor_model::{
    add_head_load_case, model_with_two_diaphragms, scale, HeadForces, HEADS, SMALL_SCALE,
    UNIFORM_MM, XY_MM,
};
use multi_floor_reference::uniform_displacement_reactions;

fn solve(model: &mut Model, name: &str, forces: HeadForces) -> StaticOnce {
    let id = add_head_load_case(model, name, forces);
    linear_static_once(model, id).unwrap()
}

fn check_equilibrium(model: &Model, response: &StaticOnce, applied: HeadForces) -> HeadForces {
    let mut external = [0.0; 3];
    let mut columns = [0.0; 3];
    let forces = std::array::from_fn(|index| {
        let (id, recorded) = &response.column_end_horizontal_forces[index];
        let element = &model.elements[index];
        assert_eq!(*id, element.id);
        let [qx, qy] = recorded.as_ref().unwrap()[1];
        let residual = element_nodal_residual(model, element, response, &[]);
        assert_relative_eq!(qx, residual[6], epsilon = 1e-9);
        assert_relative_eq!(qy, residual[7], epsilon = 1e-9);
        let [x_mm, y_mm] = XY_MM[index];
        let [fx, fy] = applied[index];
        external[0] += fx;
        external[1] += fy;
        external[2] += x_mm * fy - y_mm * fx;
        columns[0] += qx;
        columns[1] += qy;
        columns[2] += x_mm * qy - y_mm * qx + residual[11];
        [qx, qy]
    });
    for (actual, expected) in columns.into_iter().zip(external) {
        assert_relative_eq!(actual, expected, epsilon = 1e-8, max_relative = 1e-11);
    }
    let shears = response.story_shears.as_ref().unwrap();
    for axis in 0..2 {
        assert_relative_eq!(shears[axis][0], external[axis], epsilon = 1e-8);
    }
    eprintln!("  層 Fx/Fy/Mz [N, N, N·mm] = {external:.9?} / 柱集計 = {columns:.9?}");
    forces
}

fn check_uniform(model: &Model, response: &StaticOnce, reactions: HeadForces, axis: usize) {
    let forces = check_equilibrium(model, response, reactions);
    for (head, (actual, expected)) in HEADS.into_iter().zip(forces.into_iter().zip(reactions)) {
        let mut expected_disp = [0.0; 6];
        expected_disp[axis] = UNIFORM_MM;
        for (actual, expected) in response.disp[head.index()].into_iter().zip(expected_disp) {
            assert_relative_eq!(actual, expected, epsilon = 1e-11);
        }
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert_relative_eq!(actual, expected, epsilon = 1e-8);
        }
    }
}

#[test]
fn two_diaphragms_allow_local_cancellation_without_a_shared_story_rotation() {
    let mut model = model_with_two_diaphragms();
    let [uniform_x, uniform_y] = uniform_displacement_reactions();
    eprintln!("一様独立反力 X={uniform_x:.9?}, Y={uniform_y:.9?}");
    let x = solve(&mut model, "X対照", uniform_x);
    let y = solve(&mut model, "Y対照", uniform_y);
    check_uniform(&model, &x, uniform_x, 0);
    check_uniform(&model, &y, uniform_y, 1);

    let couple = solve(
        &mut model,
        "単位偶力",
        [[1.0, 0.0], [0.0; 2], [-1.0, 0.0], [0.0; 2]],
    );
    let couple_n = (SMALL_SCALE - 1.0) * UNIFORM_MM / couple.disp[HEADS[2].index()][0];
    let mut twisted_x = uniform_x;
    twisted_x[0][0] += couple_n;
    twisted_x[2][0] -= couple_n;
    eprintln!("  X偶力片側荷重 [N] = {couple_n:.9}");
    let twisted = solve(&mut model, "Xねじれ", twisted_x);
    let small_x = scale(uniform_x, SMALL_SCALE);
    let small_twisted_x = scale(twisted_x, SMALL_SCALE);
    let small_y = solve(&mut model, "Y対照縮小", scale(uniform_y, SMALL_SCALE));
    let small = solve(&mut model, "X対照縮小", small_x);
    let small_twisted = solve(&mut model, "Xねじれ縮小", small_twisted_x);

    let sample_disp = |response: &StaticOnce| {
        HEADS.map(|id| {
            let [ux, uy, _, _, _, rz] = response.disp[id.index()];
            [ux, uy, rz]
        })
    };
    let d_twisted = sample_disp(&twisted);
    for (master, slave) in [(0, 2), (1, 3)] {
        assert_relative_eq!(d_twisted[master][2], d_twisted[slave][2], epsilon = 1e-13);
        assert_relative_eq!(
            d_twisted[slave][0] - d_twisted[master][0],
            -4000.0 * d_twisted[master][2],
            epsilon = 1e-11,
        );
    }
    assert!((d_twisted[0][2] - d_twisted[1][2]).abs() > 1e-5);
    let c = 2;
    assert_relative_eq!(d_twisted[c][0], sample_disp(&small)[c][0], epsilon = 1e-11);
    let stiffness_x =
        |response| column_horizontal_stiffnesses(&model, TOP, response, SeismicDir::X).unwrap();
    let control_k = stiffness_x(&x);
    let twisted_k = stiffness_x(&twisted);
    assert_relative_eq!(control_k[c], uniform_x[c][0] / UNIFORM_MM, epsilon = 1e-8);
    assert!(twisted_k[c] > 2.0 * control_k[c]);

    let cases = [
        ("X対照", &x, uniform_x, &y),
        ("X対照縮小", &small, small_x, &small_y),
        ("Xねじれ", &twisted, twisted_x, &y),
        ("Xねじれ縮小", &small_twisted, small_twisted_x, &small_y),
    ];
    for (name, response, loads, response_y) in cases {
        eprintln!("{name} Fx/Fy [N]={loads:.9?}");
        let forces = check_equilibrium(&model, response, loads);
        let displacements = sample_disp(response);
        let [dx, dy, _] = displacements[c];
        assert_relative_eq!(
            forces[c][0],
            uniform_x[c][0] * (dx / UNIFORM_MM) + uniform_y[c][0] * (dy / UNIFORM_MM),
            epsilon = 1e-9,
        );
        let drift_x = cog_story_drifts(&model, &response.disp, 0).unwrap();
        let rs_x = stiffness_ratios(&[3000.0], &drift_x).unwrap()[0];
        let re = story_eccentricity_from_analysis(&model, TOP, response, response_y, None);
        let re_x = re.and_then(|ecc| eccentricity_ratio(ecc.ey, ecc.rex));
        let fes_x = re_x.clone().and_then(|re_x| fes(rs_x, re_x));
        eprintln!("  δx/δy/Rz [mm/mm/rad]={displacements:.12?}\n  Qx/Qy [N]={forces:.9?}");
        eprintln!(
            "  X柱比 [N/mm]={:.6?}; 層重心δx [mm]={drift_x:.9?}; Rs={rs_x}; Re={re_x:?}; Fes={fes_x:?}",
            stiffness_x(response)
        );
        let reason = re_x.unwrap_err();
        assert!(reason.contains("層上端に剛床が複数あります"), "{reason}");
        assert!(reason.contains("部材2の方向別断面剛性の軸"), "{reason}");
        assert_eq!(fes_x, Err(reason));
        assert_eq!(rs_x, 1.0);
    }

    for (large, reduced) in [(&x, &small), (&twisted, &small_twisted)] {
        for (big, tiny) in sample_disp(large)
            .into_iter()
            .flatten()
            .zip(sample_disp(reduced).into_iter().flatten())
        {
            assert_relative_eq!(big * SMALL_SCALE, tiny, epsilon = 1e-12);
        }
        for (large, reduced) in large
            .column_end_horizontal_forces
            .iter()
            .zip(&reduced.column_end_horizontal_forces)
        {
            for (big, tiny) in large.1.as_ref().unwrap()[1]
                .into_iter()
                .zip(reduced.1.as_ref().unwrap()[1])
            {
                assert_relative_eq!(big * SMALL_SCALE, tiny, epsilon = 1e-9);
            }
        }
        for (big, tiny) in stiffness_x(large).into_iter().zip(stiffness_x(reduced)) {
            assert_relative_eq!(big, tiny, epsilon = 1e-7);
        }
    }
}
