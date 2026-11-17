#[path = "../../sepika-solver/tests/support/rc_wall_side_column_model.rs"]
mod fixture;

#[test]
fn preparation_and_pushover_accept_wall_with_reference_moment_near_zero() {
    use sepika_core::dof::{Dof, Dof6Mask};
    use sepika_core::model::{ElementKind, EndCondition};
    use sepika_job::{prepare_model_for_analysis, AnalysisSettings};
    use sepika_solver::statics::analysis::{
        build_seismic_load_case_from_model, Analysis, SeismicCfg, SeismicDir,
    };
    let mut model = fixture::input_model();
    for node in &mut model.nodes[..2] {
        node.restraint.set_free(Dof::Uz);
        node.support_spring = Some([0.0, 0.0, 923_640.0, 0.0, 0.0, 0.0]);
    }
    for node in &mut model.nodes[2..] {
        node.restraint = Dof6Mask::FIXED;
        node.restraint.set_free(Dof::Ux);
        node.restraint.set_free(Dof::Uy);
    }
    model.elements[3].end_cond = [EndCondition::Pinned; 2];
    let settings = AnalysisSettings {
        push_steps: 1,
        push_apply_long_term: false,
        ..Default::default()
    };
    prepare_model_for_analysis(&mut model, &settings, None).unwrap();
    let (mut expanded, _, _) = sepika_core::wall_expand::expand_wall_elements_owned(model.clone());
    let wall = expanded
        .elements
        .iter()
        .find(|element| element.kind == ElementKind::Wall)
        .unwrap()
        .clone();
    let lateral = build_seismic_load_case_from_model(
        &expanded,
        SeismicCfg {
            dir: SeismicDir::X,
            ..Default::default()
        },
        0.06,
    )
    .unwrap();
    let case_id = lateral.id;
    expanded.load_cases = vec![lateral];
    expanded.combinations.clear();
    let result = Analysis::prepare(&expanded)
        .unwrap()
        .linear_static(case_id)
        .unwrap();
    let batch: Vec<_> = result
        .member_forces
        .iter()
        .map(|(id, force)| (*id, force.at.as_slice()))
        .collect();
    let state = sepika_element::wall::static_face::StaticWallFace::new(&wall, &expanded)
        .unwrap()
        .recover(&batch)
        .unwrap();
    assert!(state.shear_n.abs() > 1.0);
    assert!(
        state.moment_nmm.abs() / (state.shear_n.abs() * state.depth_mm) < 1e-12,
        "{state:?}"
    );
    let expected =
        sepika_element::wall::wall_element::WallElement::directional_shear_capacity_at_reference(
            &wall,
            &expanded,
            sepika_element::factory::StrengthBasis::MaterialStrength,
            0.0,
            [1.0; 2],
        )
        .unwrap();
    let result = sepika_job::compute::compute_pushover(model, settings).unwrap();
    assert_eq!(result.wall_capacities, vec![(wall.id, expected)]);
}
