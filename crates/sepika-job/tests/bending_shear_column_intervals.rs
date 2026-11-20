#[path = "fixtures/bending_shear_model.rs"]
mod bending_shear_model;

use bending_shear_model::bending_shear_response;
use sepika_core::model::Model;
use sepika_job::{build_lumped_mass, LumpedMassBuildInput};
use sepika_solver::dynamic::lumped_mass::{
    LumpedMassModel, LumpedMassType, LumpedStiffnessSource, StickDim,
};
use sepika_solver::statics::{analysis::SeismicDir, linear::StaticOnce};

fn build(
    model: &Model,
    dim: StickDim,
    response: &StaticOnce,
    response_y: Option<&StaticOnce>,
) -> sepika_job::JobResult<LumpedMassModel> {
    build_lumped_mass(LumpedMassBuildInput {
        model_type: LumpedMassType::EquivalentBendingShear,
        model,
        dim,
        source: LumpedStiffnessSource::StoryQd,
        dir: SeismicDir::X,
        nonlinear: false,
        secant_ratio: 0.75,
        target_drift_angle: 1.0 / 200.0,
        res_x: Some(response),
        res_y: response_y,
        po_x: None,
        po_y: None,
    })
}

#[test]
fn planar_bending_shear_selects_a_unique_column_interval() {
    let (mut model, mut response) = bending_shear_response(SeismicDir::X, false, true);
    let expected_ei = 1.2e14;
    for (height_mm, displacement_mm) in [(1500.0, 0.075), (2998.0, 0.15), (2.0, 0.0)] {
        model.nodes[12].coord[2] = height_mm;
        response.disp[12][2] = displacement_mm;
        let stick = build(&model, StickDim::Planar, &response, None).unwrap();
        let ei = stick.bending[0].ei_y;
        assert!(
            (ei - expected_ei).abs() < 1e-9 * expected_ei,
            "Z={height_mm} mm, EI_y={ei}"
        );
    }
}

#[test]
fn bending_shear_reports_the_layer_column_and_ambiguous_sl_boundary() {
    let (mut model, mut response_x) = bending_shear_response(SeismicDir::X, false, true);
    let (_, mut response_y) = bending_shear_response(SeismicDir::Y, false, true);
    for (height_mm, displacement_x_mm, displacement_y_mm, boundary) in [
        (2999.5, 0.15, 0.075, "上階SLの柱1"),
        (0.5, 0.0, 0.0, "下階SLの柱0"),
    ] {
        model.nodes[12].coord[2] = height_mm;
        response_x.disp[12][2] = displacement_x_mm;
        response_y.disp[12][2] = displacement_y_mm;
        for (dim, ey) in [
            (StickDim::Planar, None),
            (StickDim::Spatial, Some(&response_y)),
        ] {
            let error = build(&model, dim, &response_x, ey).unwrap_err().to_string();
            assert!(
                error.contains("階 1F")
                    && error.contains(boundary)
                    && error.contains("区間を一意に特定できません"),
                "{dim:?}: {error}"
            );
        }
    }
}
