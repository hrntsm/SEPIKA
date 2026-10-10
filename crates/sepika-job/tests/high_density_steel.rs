#[path = "fixtures/high_density_steel.rs"]
mod fixture;
use sepika_core::model::*;
use sepika_job::{
    prepare::prepare_model, weight_preparation::weights_are_current, AnalysisSettings,
};

#[test]
fn high_density_rejects_real_preparation_and_old_dl_seismic_then_recovers() {
    let mut model = fixture::steel_frame();
    let settings = AnalysisSettings::default();
    prepare_model(&mut model, &settings, None, true).unwrap();
    assert!(weights_are_current(&model, settings.mass_method));
    let initial = model.stories.clone();
    for name in [DL_CASE_NAME, EX_CASE_NAME, EY_CASE_NAME] {
        let case = model
            .load_cases
            .iter_mut()
            .find(|c| c.name == name)
            .unwrap();
        assert!(
            case.nodal.iter().any(|l| l.source == LoadSource::Auto)
                || case.member.iter().any(|l| l.source == LoadSource::Auto)
        );
        case.nodal
            .push(NodalLoad::manual(sepika_core::ids::NodeId(2), [1.0; 6]));
    }
    let manual_before: Vec<_> = model
        .load_cases
        .iter()
        .map(|case| {
            (
                case.name.clone(),
                case.nodal
                    .iter()
                    .filter(|l| l.source == LoadSource::Manual)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    model.materials[0].density = 85e-6 / 9806.65;
    model
        .load_cfg
        .get_or_insert_with(Default::default)
        .steel_weight_factor = 1.1;
    let error = prepare_model(&mut model, &settings, None, false)
        .err()
        .unwrap()
        .to_string();
    for token in ["材料 0", "要素 0", "ρg=", "78.5", "DL", "過小評価"] {
        assert!(error.contains(token), "{error}");
    }
    assert!(!weights_are_current(&model, settings.mass_method));
    for name in [DL_CASE_NAME, EX_CASE_NAME, EY_CASE_NAME] {
        let case = model.load_cases.iter().find(|c| c.name == name).unwrap();
        let expected = &manual_before.iter().find(|(n, _)| n == name).unwrap().1;
        assert_eq!(&case.nodal, expected);
        assert!(case.member.iter().all(|l| l.source != LoadSource::Auto));
        if name != DL_CASE_NAME {
            assert!(sepika_job::compute::compute_linear_static(model.clone(), case.id).is_err());
        }
    }
    let physical = model.element_mass_properties(&model.elements[0]).unwrap();
    assert!((physical.mass_per_length * 12500.0 - 0.09 * 12.5 * 8.66758781031239).abs() < 1e-10);
    model.materials[0].density = 7.85e-9;
    model.load_cfg.as_mut().unwrap().steel_weight_factor = 1.0;
    prepare_model(&mut model, &settings, None, false).unwrap();
    assert!(weights_are_current(&model, settings.mass_method));
    assert_eq!(model.stories.len(), initial.len());
}

#[test]
fn unused_high_density_and_rebar_do_not_block_common_preparation() {
    let mut model = fixture::steel_frame();
    let mut unused = model.materials[0].clone();
    unused.id = sepika_core::ids::MaterialId(3);
    unused.density = 85e-6 / 9806.65;
    model.materials.push(unused);
    model.materials[2].density = 85e-6 / 9806.65;
    prepare_model(&mut model, &Default::default(), None, true).unwrap();
}

#[test]
fn real_preparation_boundary_and_invalid_density_keep_separate_diagnostics() {
    let boundary = 78.5e-6 / 9806.65;
    for density in [7.85e-9, boundary * (1.0 - 1e-8), boundary] {
        let mut model = fixture::steel_frame();
        model.materials[0].density = density;
        prepare_model(&mut model, &Default::default(), None, true).unwrap();
    }
    for (density, reason) in [
        (boundary * (1.0 + 1e-8), "過小評価"),
        (-1e-9, "負"),
        (f64::NAN, "非有限"),
        (f64::INFINITY, "非有限"),
    ] {
        let mut model = fixture::steel_frame();
        model.materials[0].density = density;
        let error = prepare_model(&mut model, &Default::default(), None, true)
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains(reason) && error.contains("材料 0"),
            "{error}"
        );
        if reason != "過小評価" {
            assert!(!error.contains("過小評価"), "{error}");
        }
    }
}
