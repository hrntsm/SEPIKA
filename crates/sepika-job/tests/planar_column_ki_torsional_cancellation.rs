use std::f64::consts::TAU;

use sepika_core::model::{LoadCaseKind, Model};
use sepika_core::units::GRAVITY_MM_S2;
use sepika_job::compute::compute_linear_static;
use sepika_job::prepare::prepare_model_for_analysis;
use sepika_job::{build_lumped_mass, LumpedMassBuildInput};
use sepika_solver::dynamic::lumped_mass::{
    lumped_mass_eigen, LumpedMassType, LumpedStiffnessSource, StickDim,
};
use sepika_solver::statics::analysis::SeismicDir;

#[allow(dead_code)]
#[path = "../../sepika-design-jp/tests/fixtures/eccentricity_model.rs"]
mod eccentricity_model;
#[path = "../../sepika-design-jp/tests/fixtures/multi_floor_torsional_cancellation_model.rs"]
mod multi_floor_model;
#[path = "../../sepika-design-jp/tests/fixtures/multi_floor_torsional_cancellation_reference.rs"]
mod multi_floor_reference;
use eccentricity_model::{column_model, TOP};
use multi_floor_model::{
    add_head_load_case, model_with_floor, scale, Floor, HEADS, SMALL_SCALE, UNIFORM_MM, XY_MM,
};
use multi_floor_reference::uniform_displacement_reactions;

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual={actual}, expected={expected}"
    );
}

fn assert_column_hold(floor: Floor, load: &str, reason: &str, drift_reason: &str) {
    assert!(
        (0..HEADS.len()).any(|i| reason.contains(&format!("階1・X方向の柱{i}:"))),
        "{floor:?} {load}: {reason}"
    );
    assert!(reason.contains(drift_reason), "{floor:?} {load}: {reason}");
    assert!(reason.contains("生成を保留"), "{floor:?} {load}: {reason}");
}

fn prepare_comparison_mass(model: &mut Model) {
    let mut mass_model = column_model(&XY_MM, 0.0, 0.0);
    let weight = add_head_load_case(&mut mass_model, "比較用重量", [[0.0; 2]; 4]);
    let case = &mut mass_model.load_cases[weight.index()];
    case.kind = LoadCaseKind::LiveSeismic;
    for load in &mut case.nodal {
        load.values[2] = -GRAVITY_MM_S2;
    }
    prepare_model_for_analysis(&mut mass_model, &Default::default(), None).unwrap();
    model.stories[TOP.index()].dynamic_mass = mass_model.stories[TOP.index()].dynamic_mass;
}

#[test]
fn planar_column_ki_requires_compatible_column_drifts_under_proportional_loads() {
    let [uniform_x, uniform_y] = uniform_displacement_reactions();
    let uniform_k = uniform_x.iter().map(|load| load[0]).sum::<f64>() / UNIFORM_MM;
    let uniform_period = TAU * (4.0 / uniform_k).sqrt();
    let unit_couple = [[1.0, 0.0], [0.0; 2], [-1.0, 0.0], [0.0; 2]];

    for floor in [Floor::TwoDiaphragms, Floor::Shell] {
        let mut model = model_with_floor(floor);
        prepare_comparison_mass(&mut model);

        let unit = add_head_load_case(&mut model, "単位偶力", unit_couple);
        let unit_response = compute_linear_static(model.clone(), unit).unwrap();
        let c = HEADS[2].index();
        let couple_n = (SMALL_SCALE - 1.0) * UNIFORM_MM / unit_response.disp[c][0];
        let mut twisted_x = uniform_x;
        twisted_x[0][0] += couple_n;
        twisted_x[2][0] -= couple_n;

        let mut observe = |name, forces, expected_shears: [f64; 2], expected_c_drift| {
            let case = add_head_load_case(&mut model, name, forces);
            let response = compute_linear_static(model.clone(), case).unwrap();
            let shears = response.story_shears.as_ref().unwrap();
            close(shears[0][0], expected_shears[0], 1e-8);
            close(shears[1][0], expected_shears[1], 1e-8);
            close(response.disp[c][0], expected_c_drift, 1e-11);
            let result = build_lumped_mass(LumpedMassBuildInput {
                model_type: LumpedMassType::EquivalentShear,
                model: &model,
                dim: StickDim::Planar,
                source: LumpedStiffnessSource::ColumnKi,
                dir: SeismicDir::X,
                nonlinear: false,
                secant_ratio: 0.75,
                target_drift_angle: 1.0 / 200.0,
                res_x: Some(&response),
                res_y: None,
                po_x: None,
                po_y: None,
            });
            let shear_n = response.column_end_horizontal_forces[2].1.as_ref().unwrap()[1][0];
            let head_displacements_mm = HEADS.map(|head| response.disp[head.index()]);
            (result, shear_n, head_displacements_mm)
        };

        let full_shear = uniform_k * UNIFORM_MM;
        let mut twisted_column_shear = None;
        for (factor, control, twisted) in [
            (1.0, "X対照", "Xねじれ"),
            (SMALL_SCALE, "X対照縮小", "Xねじれ縮小"),
        ] {
            let expected_shears = [factor * full_shear, 0.0];
            let (result, control_shear_n, _) = observe(
                control,
                scale(uniform_x, factor),
                expected_shears,
                factor * UNIFORM_MM,
            );
            let result = result.unwrap_or_else(|err| panic!("{floor:?} {control}: {err}"));
            let [story] = result.stories.as_slice() else {
                panic!("{floor:?} {control}: 層ばねは1個必要です");
            };
            close(story.mass, 4.0, 1e-10);
            close(story.skeleton.k1, uniform_k, 1e-6);
            let modal = lumped_mass_eigen(&result, 1).unwrap();
            let [period_s] = modal.period.as_slice() else {
                panic!("{floor:?} {control}: 固有周期は1個必要です");
            };
            close(*period_s, uniform_period, 1e-10);
            close(control_shear_n, uniform_x[2][0] * factor, 1e-9);

            let (result, shear_n, _) = observe(
                twisted,
                scale(twisted_x, factor),
                expected_shears,
                factor * SMALL_SCALE * UNIFORM_MM,
            );
            assert!(shear_n.abs() > 20.0 * SMALL_SCALE * control_shear_n.abs());
            if let Some(full_column_shear) = twisted_column_shear {
                close(shear_n, factor * full_column_shear, 1e-9);
            } else {
                twisted_column_shear = Some(shear_n);
            }
            assert_column_hold(floor, twisted, &result.unwrap_err().to_string(), "層間変位");
        }

        let combined = std::array::from_fn(|i| {
            std::array::from_fn(|axis| uniform_x[i][axis] + uniform_y[i][axis])
        });
        let y_shear = uniform_y.iter().map(|load| load[1]).sum::<f64>();
        let (result, _, head_displacements_mm) =
            observe("X・Y一様並進", combined, [full_shear, y_shear], UNIFORM_MM);
        for displacements_mm in head_displacements_mm {
            close(displacements_mm[0], UNIFORM_MM, 1e-11);
            close(displacements_mm[1], UNIFORM_MM, 1e-11);
        }
        assert_column_hold(
            floor,
            "X・Y一様並進",
            &result.unwrap_err().to_string(),
            "直交方向に層間変位",
        );
    }
}
