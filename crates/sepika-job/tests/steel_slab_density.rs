#[path = "fixtures/steel_slab.rs"]
mod fixture;
use sepika_core::{ids::*, model::*};
use sepika_job::{auto_loads, prepare::prepare_model, weight_preparation::weights_are_current};

fn total(loads: &[sepika_load::floor::BeamLoad]) -> f64 {
    loads.iter().map(|load| load.cmq.q_i + load.cmq.q_j).sum()
}

#[test]
fn steel_slab_one_cubic_metre_design_and_independent_physical_distribution() {
    for attached in [false, true] {
        let mut model = fixture::one_cubic_metre(attached);
        let slab = &model.slabs[0];
        assert_eq!(model.slab_plate_thickness(slab), Some(100.0));
        assert!((model.slab_self_weight_intensity(slab).unwrap() - 0.00785).abs() < 1e-12);
        assert!(
            (total(&auto_loads::compute_dl_beam_loads(&model).unwrap()) - 78500.0).abs() < 1e-8
        );
        // 材料から質量を生成するAPIではなく、独立に与えた物理面荷重の分配を検証する。
        let physical =
            sepika_load::floor::distribute_slab_w_checked(&model, slab, 0.00769822025).unwrap();
        assert!((total(&physical) - 76982.2025).abs() < 1e-8);
        model.materials[1].density = 85e-6 / 9806.65;
        let physical =
            sepika_load::floor::distribute_slab_w_checked(&model, &model.slabs[0], 0.0085).unwrap();
        assert!((total(&physical) - 85000.0).abs() < 1e-8);
        assert!((total(&physical) - 78500.0 - 6500.0).abs() < 1e-8);
        for error in [
            auto_loads::compute_dl_beam_loads(&model)
                .unwrap_err()
                .to_string(),
            sepika_load::floor::distribute_slab(&model, &model.slabs[0])
                .unwrap_err()
                .to_string(),
        ] {
            for token in [
                "材料 1",
                "床板 0",
                "入力ρ=",
                "ρg=",
                "78.5",
                "DL",
                "過小評価",
            ] {
                assert!(error.contains(token), "{error}");
            }
        }
        let ll =
            sepika_load::floor::distribute_slab_w_checked(&model, &model.slabs[0], 0.003).unwrap();
        assert!((total(&ll) - 30000.0).abs() < 1e-8);
    }
}

#[test]
fn steel_slab_density_boundary_invalid_inputs_unused_shared_and_finish() {
    let boundary = 78.5e-6 / 9806.65;
    for density in [0.0, 7.85e-9, boundary * (1.0 - 1e-8), boundary] {
        let mut model = fixture::one_cubic_metre(true);
        model.materials[1].density = density;
        auto_loads::compute_dl_beam_loads(&model).unwrap();
    }
    for (density, reason) in [
        (boundary * (1.0 + 1e-8), "過小評価"),
        (-1e-9, "負"),
        (f64::NAN, "非有限"),
        (f64::INFINITY, "非有限"),
        (f64::NEG_INFINITY, "非有限"),
    ] {
        let mut model = fixture::one_cubic_metre(true);
        model.materials[1].density = density;
        let error = auto_loads::compute_dl_beam_loads(&model)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(reason) && error.contains("床板 0"),
            "{error}"
        );
        if reason != "過小評価" {
            assert!(!error.contains("過小評価"), "{error}");
        }
    }
    let mut model = fixture::one_cubic_metre(true);
    let mut unused = model.materials[1].clone();
    unused.id = MaterialId(3);
    unused.density = 85e-6 / 9806.65;
    model.materials.push(unused);
    auto_loads::compute_dl_beam_loads(&model).unwrap();
    let mut shared = model.slabs[0].clone();
    shared.id = SlabId(1);
    model.slabs.push(shared);
    model.slabs[0].plate.loads.push(AreaLoad {
        kind: "仕上げ".into(),
        value: 0.01,
    });
    model.materials[1].density = 85e-6 / 9806.65;
    assert!(auto_loads::compute_gravity_auto_load_cases(&model).is_err());
    model.slabs.remove(0);
    let error = auto_loads::compute_dl_beam_loads(&model)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("材料 1") && error.contains("床板 1"),
        "{error}"
    );
}

#[test]
fn steel_slab_missing_references_and_geometry_keep_their_diagnostics() {
    let mut model = fixture::one_cubic_metre(true);
    model.materials[1].density = 85e-6 / 9806.65;
    model.slabs[0].plate.section = None;
    let error = auto_loads::compute_dl_beam_loads(&model)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("断面が未割当") && !error.contains("過小評価"),
        "{error}"
    );
    model.slabs[0].plate.section = Some(SectionId(2));
    model.sections[2].thickness = None;
    let error = auto_loads::compute_dl_beam_loads(&model)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("材料または板厚") && !error.contains("過小評価"),
        "{error}"
    );
    model.sections[2].thickness = Some(100.0);
    model.sections[2].material = None;
    assert!(auto_loads::compute_dl_beam_loads(&model)
        .unwrap_err()
        .to_string()
        .contains("材料または板厚"));
    model.sections[2].material = Some(MaterialId(1));
    if let SlabShape::Attached { extent, .. } = &mut model.slabs[0].shape {
        *extent = [f64::NAN; 2];
    }
    let error = auto_loads::compute_dl_beam_loads(&model)
        .unwrap_err()
        .to_string();
    assert!(!error.contains("過小評価"), "{error}");
}

#[test]
fn steel_slab_preparation_invalidates_old_auto_preserves_manual_and_recovers() {
    let mut model = fixture::one_cubic_metre(true);
    let settings = Default::default();
    prepare_model(&mut model, &settings, None, true).unwrap();
    assert!(weights_are_current(&model, settings.mass_method));
    for name in [DL_CASE_NAME, EX_CASE_NAME, EY_CASE_NAME] {
        let case = model
            .load_cases
            .iter_mut()
            .find(|c| c.name == name)
            .unwrap();
        case.nodal.push(NodalLoad::manual(NodeId(2), [1.0; 6]));
    }
    model.materials[1].density = 85e-6 / 9806.65;
    assert!(prepare_model(&mut model, &settings, None, false).is_err());
    assert!(!weights_are_current(&model, settings.mass_method));
    for name in [DL_CASE_NAME, EX_CASE_NAME, EY_CASE_NAME] {
        let case = model.load_cases.iter().find(|c| c.name == name).unwrap();
        assert_eq!(case.nodal, vec![NodalLoad::manual(NodeId(2), [1.0; 6])]);
        assert!(case.member.iter().all(|l| l.source != LoadSource::Auto));
    }
    model.materials[1].density = 7.85e-9;
    prepare_model(&mut model, &settings, None, false).unwrap();
    assert!(weights_are_current(&model, settings.mass_method));
}

#[test]
fn steel_slab_synced_story_entry_rejects_high_density_but_cases_only_remains_independent() {
    let mut model = fixture::one_cubic_metre(true);
    prepare_model(&mut model, &Default::default(), None, true).unwrap();
    let cases = sepika_job::gravity_case_ids_for_seismic_weight(&model);
    let before = sepika_load::story_gen::generate_stories_with_synced_self_weight(
        &model,
        &cases,
        Default::default(),
    )
    .unwrap();
    assert!(
        (before
            .stories
            .iter()
            .map(|s| s.seismic_weight.unwrap_or(0.0))
            .sum::<f64>()
            - 78500.0)
            .abs()
            < 1e-8
    );
    model.materials[1].density = 85e-6 / 9806.65;
    let error = sepika_load::story_gen::generate_stories_with_synced_self_weight(
        &model,
        &cases,
        Default::default(),
    )
    .err()
    .unwrap();
    assert!(
        error.contains("床板 0") && error.contains("過小評価"),
        "{error}"
    );
    sepika_load::story_gen::generate_stories_with_opts(&model, &cases, false, Default::default())
        .unwrap();
}

#[test]
fn steel_slab_design_guard_uses_stb_material_priority_without_section_fallback() {
    let mut model = fixture::one_cubic_metre(true);
    model.materials[1].density = 85e-6 / 9806.65;
    model.stb_strengths.members.push(StbMemberStrength {
        target: StrengthTarget::Slab(SlabId(0)),
        node: NodeId(2),
        node_order: vec![NodeId(2), NodeId(3)],
        concrete: Some("Fc24".into()),
    });
    model.prepare_stb_strength_materials();
    let material = model.slab_plate_material(&model.slabs[0]).unwrap();
    assert_eq!(material.category, MaterialCategory::Concrete);
    assert_ne!(material.id, MaterialId(1));
    auto_loads::compute_dl_beam_loads(&model).unwrap();
    model.stb_strengths.materials.clear();
    let error = auto_loads::compute_dl_beam_loads(&model)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("材料または板厚") && !error.contains("過小評価"),
        "{error}"
    );
}
