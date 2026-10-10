#[path = "fixtures/seismic_freshness.rs"]
mod fixture;
use sepika_core::model::*;
use sepika_job::{prepare::prepare_model, weight_preparation::*, AnalysisSettings};

#[test]
fn common_preparation_matches_independent_two_storeys_and_preserves_user_settings() {
    let mut model = fixture::two_storeys();
    let settings = AnalysisSettings::default();
    prepare_model(&mut model, &settings, None, true).unwrap();
    assert!(weights_are_current(&model, settings.mass_method));
    let weights: Vec<_> = model.layers().iter().map(|l| l.weight.unwrap()).collect();
    assert!(
        weights
            .iter()
            .all(|weight| (*weight - 100000.0).abs() < 1e-8),
        "{weights:?}"
    );
    for (name, axis) in [(EX_CASE_NAME, 0), (EY_CASE_NAME, 1)] {
        let case = model.load_cases.iter().find(|c| c.name == name).unwrap();
        let forces: Vec<_> = case.nodal.iter().map(|l| l.values[axis]).collect();
        assert!((forces[0] - 12686.2915010152).abs() < 1e-8);
        assert!((forces[1] - 27313.7084989848).abs() < 1e-8);
        assert!((forces.iter().sum::<f64>() - 40000.0).abs() < 1e-8);
    }
    let key = weight_input_key(&model, settings.mass_method);
    let output = weight_output_key(&model);
    let record = model.seismic_weight_generation.clone();
    model.stories[1].name = "任意階名".into();
    model.materials[0].name = "表示名称".into();
    assert_eq!(weight_input_key(&model, settings.mass_method), key);
    prepare_model(&mut model, &settings, None, false).unwrap();
    assert_eq!(weight_output_key(&model), output);
    assert_eq!(model.seismic_weight_generation, record);
    model.materials[0].density = 2.6e-9;
    prepare_model(&mut model, &settings, None, false).unwrap();
    for (layer, expected) in model
        .layers()
        .iter()
        .zip(fixture::expected_weights(2.6e-9, 0.0, 0.0))
    {
        assert!((layer.weight.unwrap() - expected).abs() < 1e-8);
    }
    model.stories[1].weight_override = Some(150000.0);
    let elevation = model.stories[1].elevation;
    prepare_model(&mut model, &settings, None, false).unwrap();
    assert_eq!(model.stories[1].seismic_weight, Some(150000.0));
    assert_eq!(model.stories[1].name, "任意階名");
    assert_eq!(model.stories[1].elevation, elevation);
    assert_ne!(
        model
            .seismic_weight_generation
            .as_ref()
            .unwrap()
            .calculated_weights[1]
            .1,
        150000.0
    );
    model.stories[1].seismic_weight = Some(1.0);
    assert!(!weights_are_current(&model, settings.mass_method));
    prepare_model(&mut model, &settings, None, false).unwrap();
    assert_eq!(model.stories[1].seismic_weight, Some(150000.0));
}

#[test]
fn input_key_excludes_generated_outputs_and_retains_manual_mass_and_section_floor() {
    let mut model = fixture::two_storeys();
    let settings = AnalysisSettings::default();
    prepare_model(&mut model, &settings, None, true).unwrap();
    let key = weight_input_key(&model, settings.mass_method);
    model.assign_stb_node_ids().unwrap();
    assert_eq!(weight_input_key(&model, settings.mass_method), key);
    model.stories[1].seismic_weight = Some(123.0);
    model.stories[1].dynamic_mass = None;
    let master = model.generated_masters[0];
    model.nodes[master.index()].mass = Some([987.0; 6]);
    model.nodes[master.index()].coord[0] += 50.0;
    assert_eq!(weight_input_key(&model, settings.mass_method), key);
    assert!(!weights_are_current(&model, settings.mass_method));
    model.nodes[2].mass = Some([10.0; 6]);
    assert_ne!(weight_input_key(&model, settings.mass_method), key);
    let key = weight_input_key(&model, settings.mass_method);
    model.sections[0].floor = Some("材料解決階".into());
    assert_ne!(weight_input_key(&model, settings.mass_method), key);
}

#[test]
fn user_diaphragm_mass_and_coefficient_survive_weight_refresh() {
    let mut model = fixture::two_storeys();
    let settings = AnalysisSettings::default();
    prepare_model(&mut model, &settings, None, true).unwrap();
    let master = model.generated_masters[0];
    model.generated_masters.retain(|id| *id != master);
    model.nodes[master.index()].mass = Some([42.0; 6]);
    let constraint = model
        .constraints
        .iter_mut()
        .find(|c| {
            matches!(c,
        Constraint::RigidDiaphragm { master: id, .. } if *id == master)
        })
        .unwrap();
    if let Constraint::RigidDiaphragm {
        ci_override,
        weight,
        ..
    } = constraint
    {
        *ci_override = Some(0.3);
        *weight = Some(100000.0);
    }
    model.materials[0].density = 2.6e-9;
    prepare_model(&mut model, &settings, None, false).unwrap();
    assert_eq!(model.nodes[master.index()].mass, Some([42.0; 6]));
    assert!(model.constraints.iter().any(|c| matches!(c,
        Constraint::RigidDiaphragm { master: id, ci_override: Some(0.3), weight: Some(100000.0), .. } if *id == master)));
}

#[test]
fn invalid_seismic_weights_clear_both_auto_directions_without_rejecting_independent_dl() {
    for invalid in [-1.0, 0.0, f64::NAN, f64::INFINITY] {
        let mut model = fixture::two_storeys();
        let settings = AnalysisSettings::default();
        prepare_model(&mut model, &settings, None, true).unwrap();
        let ex = model
            .load_cases
            .iter()
            .find(|case| case.name == EX_CASE_NAME)
            .unwrap()
            .id;
        let manual = NodalLoad::manual(sepika_core::ids::NodeId(2), [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        model.load_cases[ex.index()].nodal.push(manual.clone());
        model.stories[1].weight_override = Some(invalid);
        let report = prepare_model(&mut model, &settings, None, false).unwrap();
        assert!(
            report.notices.iter().any(|notice| notice.contains("重量")),
            "{:?}",
            report.notices
        );
        for name in [EX_CASE_NAME, EY_CASE_NAME] {
            let case = model
                .load_cases
                .iter()
                .find(|case| case.name == name)
                .unwrap();
            assert!(sepika_job::compute::missing_seismic_horizontal_load(case));
        }
        assert!(model.load_cases[ex.index()].nodal.contains(&manual));
        assert!(sepika_job::compute::compute_linear_static(model.clone(), ex).is_err());
        let dl = model
            .load_cases
            .iter()
            .find(|case| case.name == DL_CASE_NAME)
            .unwrap()
            .id;
        assert!(sepika_job::compute::compute_linear_static(model, dl).is_ok());
    }
}

#[test]
fn multiple_diaphragms_with_ambiguous_ownership_reject_both_directions() {
    let mut model = fixture::two_storeys();
    let settings = AnalysisSettings::default();
    prepare_model(&mut model, &settings, None, true).unwrap();
    let mut duplicate = model
        .constraints
        .iter()
        .find(|constraint| {
            matches!(constraint,
        Constraint::RigidDiaphragm { story, .. } if story.0 == 1)
        })
        .unwrap()
        .clone();
    if let Constraint::RigidDiaphragm { ci_override, .. } = &mut duplicate {
        *ci_override = Some(0.3);
    }
    model.constraints.push(duplicate);
    let report = prepare_model(&mut model, &settings, None, false).unwrap();
    assert!(report
        .notices
        .iter()
        .any(|notice| notice.contains("一意に帰属できません")));
    let error = sepika_job::compute::compute_eigen(model.clone(), 1).unwrap_err();
    assert!(error
        .to_string()
        .contains("物理質量が現在入力と一致しません"));
    let wave = sepika_solver::dynamic::timehistory::GroundMotion {
        dt: 0.01,
        accel_x: vec![0.0; 2],
        accel_y: None,
        accel_theta: None,
    };
    let error =
        sepika_job::compute::compute_time_history(model.clone(), settings, wave).unwrap_err();
    assert!(error
        .to_string()
        .contains("物理質量が現在入力と一致しません"));
    let dl = model
        .load_cases
        .iter()
        .find(|case| case.name == DL_CASE_NAME)
        .unwrap()
        .id;
    assert!(sepika_job::compute::compute_linear_static(model.clone(), dl).is_ok());
    for name in [EX_CASE_NAME, EY_CASE_NAME] {
        assert!(sepika_job::compute::missing_seismic_horizontal_load(
            model
                .load_cases
                .iter()
                .find(|case| case.name == name)
                .unwrap()
        ));
    }
}

#[test]
fn legacy_saved_automatic_floor_includes_added_geometry_and_keeps_stable_generation() {
    let mut model = fixture::two_storeys();
    let settings = AnalysisSettings::default();
    prepare_model(&mut model, &settings, None, true).unwrap();
    model.seismic_weight_generation = None;
    let mut node = model.nodes[2].clone();
    node.id = sepika_core::ids::NodeId(model.nodes.len() as u32);
    node.coord[1] = 2000.0;
    let added = node.id;
    model.nodes.push(node);
    let mut beam = model.elements[4].clone();
    beam.id = sepika_core::ids::ElemId(model.elements.len() as u32);
    beam.nodes = [sepika_core::ids::NodeId(2), added].into_iter().collect();
    model.elements.push(beam);
    prepare_model(&mut model, &settings, None, false).unwrap();
    assert!(model
        .diaphragms_of(sepika_core::ids::StoryId(1))
        .any(|d| d.slaves.contains(&added)));
    assert!(weights_are_current(&model, settings.mass_method));
    let record = model.seismic_weight_generation.clone();
    prepare_model(&mut model, &settings, None, false).unwrap();
    assert_eq!(model.seismic_weight_generation, record);
}

#[test]
fn generated_master_restraints_follow_released_structure_and_preserve_user_edits() {
    use sepika_core::dof::{Dof, Dof6Mask};
    let mut model = fixture::two_storeys();
    let settings = AnalysisSettings::default();
    for node in &mut model.nodes[2..6] {
        node.restraint.set_fixed(Dof::Uy);
    }
    prepare_model(&mut model, &settings, None, true).unwrap();
    let upper = model.generated_masters[2];
    assert!(model.nodes[upper.index()].restraint.is_fixed(Dof::Uy));
    for node in &mut model.nodes[2..6] {
        node.restraint = Dof6Mask::FREE;
    }
    prepare_model(&mut model, &settings, None, false).unwrap();
    assert!(!model.nodes[upper.index()].restraint.is_fixed(Dof::Uy));
    let ey = model
        .load_cases
        .iter()
        .find(|case| case.name == EY_CASE_NAME)
        .unwrap();
    assert!((ey.nodal.iter().map(|load| load.values[1]).sum::<f64>() - 40000.0).abs() < 1e-8);
    let result = sepika_job::compute::compute_linear_static(model.clone(), ey.id).unwrap();
    assert!(result.disp.iter().flatten().any(|value| value.abs() > 1e-8));
    let record = model.seismic_weight_generation.clone();
    prepare_model(&mut model, &settings, None, false).unwrap();
    assert_eq!(model.seismic_weight_generation, record);

    model.nodes[upper.index()].restraint.set_fixed(Dof::Uy);
    let manual_key = weight_input_key(&model, settings.mass_method);
    model.materials[0].density = 2.6e-9;
    prepare_model(&mut model, &settings, None, false).unwrap();
    assert!(model.generated_masters.contains(&upper));
    assert!(model.nodes[upper.index()].restraint.is_fixed(Dof::Uy));
    assert_ne!(weight_input_key(&model, settings.mass_method), manual_key);
    assert!(!model.is_automatic_seismic_master_restraint(upper));
    let ey = model
        .load_cases
        .iter()
        .find(|case| case.name == EY_CASE_NAME)
        .unwrap()
        .id;
    assert!(
        sepika_job::compute::compute_linear_static(model.clone(), ey)
            .unwrap_err()
            .to_string()
            .contains("水平拘束")
    );
    let record = model.seismic_weight_generation.clone();
    prepare_model(&mut model, &settings, None, false).unwrap();
    assert_eq!(model.seismic_weight_generation, record);
}

#[test]
fn edited_generated_diaphragm_weight_and_ci_survive_density_refresh() {
    for ci in [Some(0.3), None] {
        let mut model = fixture::two_storeys();
        let settings = AnalysisSettings::default();
        prepare_model(&mut model, &settings, None, true).unwrap();
        let upper = model.generated_masters[2];
        let key = weight_input_key(&model, settings.mass_method);
        let manual_weight = if ci.is_some() { 100000.0 } else { 100100.0 };
        let constraint = model
            .constraints
            .iter_mut()
            .find(|constraint| {
                matches!(constraint,
            Constraint::RigidDiaphragm {master, ..} if *master == upper)
            })
            .unwrap();
        if let Constraint::RigidDiaphragm {
            weight,
            ci_override,
            ..
        } = constraint
        {
            *weight = Some(manual_weight);
            *ci_override = ci;
        }
        assert_ne!(weight_input_key(&model, settings.mass_method), key);
        model.materials[0].density = 2.6e-9;
        let report = prepare_model(&mut model, &settings, None, false).unwrap();
        assert!(model.generated_masters.contains(&upper));
        assert!(model
            .constraints
            .iter()
            .any(|constraint| matches!(constraint,
            Constraint::RigidDiaphragm {master, weight: Some(weight), ci_override, ..}
            if *master == upper && *weight == manual_weight && *ci_override == ci)));
        let expected = fixture::expected_weights(2.6e-9, 0.0, 0.0);
        for (layer, expected) in model.layers().iter().zip(expected) {
            assert!((layer.weight.unwrap() - expected).abs() < 1e-8);
        }
        if ci.is_some() {
            assert!(
                report.notices.iter().any(|notice| notice.contains("合力")),
                "{:?}",
                report.notices
            );
            for name in [EX_CASE_NAME, EY_CASE_NAME] {
                let case = model
                    .load_cases
                    .iter()
                    .find(|case| case.name == name)
                    .unwrap();
                assert!(sepika_job::compute::missing_seismic_horizontal_load(case));
            }
            let dl = model
                .load_cases
                .iter()
                .find(|case| case.name == DL_CASE_NAME)
                .unwrap()
                .id;
            sepika_job::compute::compute_linear_static(model.clone(), dl).unwrap();
        } else {
            assert!(
                !report
                    .notices
                    .iter()
                    .any(|notice| notice.contains("Ai 地震力")),
                "{:?}",
                report.notices
            );
            let total = 0.2 * expected.iter().sum::<f64>();
            for (name, axis) in [(EX_CASE_NAME, 0), (EY_CASE_NAME, 1)] {
                let case = model
                    .load_cases
                    .iter()
                    .find(|case| case.name == name)
                    .unwrap();
                assert!(
                    (case.nodal.iter().map(|load| load.values[axis]).sum::<f64>() - total).abs()
                        < 1e-8
                );
            }
        }
        let record = model.seismic_weight_generation.clone();
        prepare_model(&mut model, &settings, None, false).unwrap();
        assert_eq!(model.seismic_weight_generation, record);
    }
}
