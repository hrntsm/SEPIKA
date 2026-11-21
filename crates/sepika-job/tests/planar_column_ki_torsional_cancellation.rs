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
fn planar_column_ki_shows_a_load_dependent_stiffness_when_column_drift_cancels() {
    let [uniform_x, _] = uniform_displacement_reactions();
    let uniform_k = uniform_x.iter().map(|load| load[0]).sum::<f64>() / UNIFORM_MM;
    let uniform_period = TAU * (4.0 / uniform_k).sqrt();
    let unit_couple = [[1.0, 0.0], [0.0; 2], [-1.0, 0.0], [0.0; 2]];

    for (floor, minimum_stiffness_ratio) in [(Floor::TwoDiaphragms, 5.0), (Floor::Shell, 15.0)] {
        let mut model = model_with_floor(floor);
        prepare_comparison_mass(&mut model);

        let unit = add_head_load_case(&mut model, "単位偶力", unit_couple);
        let unit_response = compute_linear_static(model.clone(), unit).unwrap();
        let c = HEADS[2].index();
        let couple_n = (SMALL_SCALE - 1.0) * UNIFORM_MM / unit_response.disp[c][0];
        let mut twisted_x = uniform_x;
        twisted_x[0][0] += couple_n;
        twisted_x[2][0] -= couple_n;

        let mut observe = |name, forces, expected_shear, expected_c_drift| {
            let case = add_head_load_case(&mut model, name, forces);
            let response = compute_linear_static(model.clone(), case).unwrap();
            let shears = response.story_shears.as_ref().unwrap();
            close(shears[0][0], expected_shear, 1e-8);
            close(shears[1][0], 0.0, 1e-8);
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
            })
            .unwrap_or_else(|err| panic!("{floor:?} {name}: {err}"));
            let [story] = result.stories.as_slice() else {
                panic!("{floor:?} {name}: 層ばねは1個必要です");
            };
            close(story.mass, 4.0, 1e-10);
            let modal = lumped_mass_eigen(&result, 1).unwrap();
            let [period_s] = modal.period.as_slice() else {
                panic!("{floor:?} {name}: 固有周期は1個必要です");
            };
            let shear_n = response.column_end_horizontal_forces[2].1.as_ref().unwrap()[1][0];
            eprintln!("{floor:?} {name}: 層せん断力={:.9} N、柱C変位={:.12} mm、柱C負担力={shear_n:.9} N、質量={:.9} t、K={:.9} N/mm、T={period_s:.9} s",
                shears[0][0], response.disp[c][0], story.mass, story.skeleton.k1);
            (story.skeleton.k1, *period_s, shear_n)
        };

        let full_shear = uniform_k * UNIFORM_MM;
        let small_drift = SMALL_SCALE * UNIFORM_MM;
        let small_shear = SMALL_SCALE * full_shear;
        let (uniform, period, _) = observe("X対照", uniform_x, full_shear, UNIFORM_MM);
        let (small, small_period, small_column_shear) = observe(
            "X対照縮小",
            scale(uniform_x, SMALL_SCALE),
            small_shear,
            small_drift,
        );
        let (twisted, twisted_period, twisted_column_shear) =
            observe("Xねじれ", twisted_x, full_shear, small_drift);
        let (small_twisted, small_twisted_period, _) = observe(
            "Xねじれ縮小",
            scale(twisted_x, SMALL_SCALE),
            small_shear,
            small_drift * SMALL_SCALE,
        );

        for result in [uniform, small] {
            close(result, uniform_k, 1e-6);
        }
        for result in [period, small_period] {
            close(result, uniform_period, 1e-10);
        }
        close(small_column_shear, uniform_x[2][0] * SMALL_SCALE, 1e-9);
        assert!(twisted_column_shear > 20.0 * small_column_shear);
        assert!(twisted > minimum_stiffness_ratio * uniform_k);
        assert!(twisted_period < uniform_period / minimum_stiffness_ratio.sqrt());
        close(small_twisted, twisted, 1e-6);
        close(small_twisted_period, twisted_period, 1e-10);
    }
}
