#[path = "fixtures/high_density_steel_model.rs"]
mod fixture;

use sepika_core::{
    dof::{Dof, Dof6Mask},
    ids::*,
    model::*,
};
use sepika_job::{
    compute::{compute_eigen, compute_linear_static, compute_lumped_mass},
    prepare::prepare_model,
    AnalysisSettings,
};

fn two_columns(factor: f64, method: MassMethod) -> Model {
    let mut model = fixture::steel_frame();
    model.stories.truncate(2);
    model.stories[1].elevation = Some(3000.0);
    model.nodes.truncate(4);
    for node in &mut model.nodes[2..] {
        node.coord[2] = 3000.0;
        node.restraint = Dof6Mask::FIXED;
        node.restraint.set_free(Dof::Ux);
        node.restraint.set_free(Dof::Uy);
    }
    model.elements.truncate(2);
    model.slab_composite_attrs.clear();
    model.materials.truncate(1);
    model.sections.truncate(1);
    model.sections[0].name = "鋼柱".into();
    model.sections[0].area = 10000.0;
    model.sections[0].iy = 1e6;
    model.sections[0].iz = 1e6;
    model.sections[0].as_y = 10000.0;
    model.sections[0].as_z = 10000.0;
    model.slabs.clear();
    model.load_cases.clear();
    model
        .load_cfg
        .get_or_insert_with(Default::default)
        .steel_weight_factor = factor;
    model.load_cases.push(LoadCase {
        origin: Default::default(),
        id: LoadCaseId(0),
        name: "水平単位荷重".into(),
        kind: LoadCaseKind::Other,
        nodal: vec![NodalLoad::manual(
            NodeId(2),
            [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        )],
        member: vec![],
    });
    prepare_model(
        &mut model,
        &AnalysisSettings {
            mass_method: method,
            ..Default::default()
        },
        None,
        false,
    )
    .unwrap();
    model
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-8 * expected.abs().max(1.0),
        "actual={actual}, expected={expected}"
    );
}

#[test]
fn steel_factor_below_unity_exposes_clipped_mass_and_distinct_dynamic_response() {
    for method in [MassMethod::LumpedOnly, MassMethod::CorrectedLumped] {
        let reference = two_columns(1.0, method);
        let reference_eigen = compute_eigen(reference.clone(), 1).unwrap();
        let reference_ex = reference
            .load_cases
            .iter()
            .find(|lc| lc.name == EX_CASE_NAME)
            .unwrap();
        let reference_seismic = compute_linear_static(reference.clone(), reference_ex.id).unwrap();
        let reference_dl = reference
            .load_cases
            .iter()
            .find(|lc| lc.name == DL_CASE_NAME)
            .unwrap();
        let reference_gravity = compute_linear_static(reference.clone(), reference_dl.id).unwrap();
        for factor in [0.25, 0.5, 1.0, 1.5] {
            let model = two_columns(factor, method);
            let mass_t = 0.03 * 7.85;
            let element_mass_t: f64 = model
                .elements
                .iter()
                .map(|element| {
                    let actual = model
                        .element_mass_properties(element)
                        .unwrap()
                        .mass_per_length
                        * 3000.0;
                    close(actual, mass_t);
                    actual
                })
                .sum();
            let total_weight: f64 = model
                .stories
                .iter()
                .filter_map(|s| s.dynamic_mass)
                .map(|m| m.mass_equiv_weight_n)
                .sum();
            close(total_weight, 2.0 * mass_t * 9806.65 * factor);
            let layer_mass: f64 = model
                .layers()
                .iter()
                .filter_map(|l| l.dynamic_mass)
                .map(|m| m.mass_equiv_weight_n / 9806.65)
                .sum();
            close(layer_mass, mass_t * factor);
            let lumped_t: f64 = model
                .nodes
                .iter()
                .filter_map(|n| n.mass)
                .map(|m| m[0])
                .sum();
            let expected_lumped_t = match method {
                MassMethod::LumpedOnly => 2.0 * mass_t * factor,
                MassMethod::CorrectedLumped if factor < 1.0 => 0.0,
                MassMethod::CorrectedLumped => 2.0 * mass_t * (factor - 1.0),
            };
            close(lumped_t, expected_lumped_t);
            let nominal_t = lumped_t
                + if method == MassMethod::CorrectedLumped {
                    element_mass_t
                } else {
                    0.0
                };
            let clipped_t = if method == MassMethod::CorrectedLumped && factor < 1.0 {
                2.0 * mass_t * (1.0 - factor)
            } else {
                0.0
            };
            close(nominal_t - total_weight / 9806.65, clipped_t);
            for story in &model.stories {
                let mass = story.dynamic_mass.unwrap();
                close(mass.mass_equiv_weight_n / 9806.65, mass_t * factor);
                close(mass.center_xy_mm[0], 3000.0);
                close(mass.inertia_t_mm2, mass_t * factor * 3000.0_f64.powi(2));
            }
            let static_result = compute_linear_static(model.clone(), LoadCaseId(0)).unwrap();
            close(
                static_result.disp[2][0],
                500.0
                    * (3000.0_f64.powi(3) / (12.0 * 205000.0 * 1e6)
                        + 3000.0 / (205000.0 / 2.4 * 10000.0)),
            );
            for (_, forces) in &static_result.member_forces {
                close(forces.at[0].1[1], -500.0);
                close(forces.at[0].1[5], 750000.0);
                close(forces.at[2].1[5], -750000.0);
            }
            let dl = model
                .load_cases
                .iter()
                .find(|lc| lc.name == DL_CASE_NAME)
                .unwrap();
            let gravity = compute_linear_static(model.clone(), dl.id).unwrap();
            for ((_, actual), (_, reference)) in gravity
                .member_forces
                .iter()
                .zip(&reference_gravity.member_forces)
            {
                for ((_, actual), (_, reference)) in actual.at.iter().zip(&reference.at) {
                    for (actual, reference) in actual.iter().zip(reference) {
                        close(*actual, reference * factor);
                    }
                }
            }
            close(
                model.stories.iter().filter_map(|s| s.seismic_weight).sum(),
                0.06 * 78500.0 * factor,
            );
            let ex = model
                .load_cases
                .iter()
                .find(|lc| lc.name == EX_CASE_NAME)
                .unwrap();
            close(
                ex.nodal.iter().map(|n| n.values[0]).sum(),
                reference_ex.nodal.iter().map(|n| n.values[0]).sum::<f64>() * factor,
            );
            let seismic = compute_linear_static(model.clone(), ex.id).unwrap();
            close(seismic.disp[2][0], reference_seismic.disp[2][0] * factor);
            let stick = compute_lumped_mass(
                model.clone(),
                AnalysisSettings {
                    mass_method: method,
                    ..Default::default()
                },
                Some(seismic),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            close(
                stick.model.stories.iter().map(|s| s.mass).sum(),
                mass_t * factor,
            );
            close(
                stick.modal.period[0],
                2.0 * std::f64::consts::PI
                    * (mass_t
                        * factor
                        * (3000.0_f64.powi(3) / (24.0 * 205000.0 * 1e6)
                            + 3000.0 / (2.0 * 205000.0 / 2.4 * 10000.0)))
                        .sqrt(),
            );
            let eigen = compute_eigen(model.clone(), 1).unwrap();
            match method {
                MassMethod::LumpedOnly => {
                    close(eigen.period[0] / reference_eigen.period[0], factor.sqrt())
                }
                MassMethod::CorrectedLumped if factor <= 1.0 => {
                    close(eigen.period[0], reference_eigen.period[0])
                }
                MassMethod::CorrectedLumped => assert!(eigen.period[0] > reference_eigen.period[0]),
            }
            println!("{method:?} f={factor}: nominal={nominal_t:.8} t, all_floors={:.8} t, layer={layer_mass:.8} t, clipped={clipped_t:.8} t, T={:.10} s", total_weight / 9806.65, eigen.period[0]);
        }
    }
}
