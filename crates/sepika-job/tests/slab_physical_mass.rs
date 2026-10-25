#[path = "fixtures/steel_slab.rs"]
mod fixture;
use sepika_core::{ids::*, model::*};
use sepika_job::{auto_loads, prepare::prepare_model, weight_preparation::weights_are_current};

fn physical_weight(model: &Model) -> f64 {
    model
        .stories
        .iter()
        .filter_map(|s| s.dynamic_mass)
        .map(|m| m.mass_equiv_weight_n)
        .sum()
}
fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-7,
        "actual={actual}, expected={expected}"
    );
}
fn load_total(loads: &[sepika_load::floor::BeamLoad]) -> f64 {
    loads.iter().map(|l| l.cmq.q_i + l.cmq.q_j).sum()
}

#[test]
fn one_cubic_metre_slab_replaces_design_weight_once_for_both_shapes_and_mass_methods() {
    for attached in [false, true] {
        for mass_method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
            let mut model = fixture::one_cubic_metre(attached);
            let settings = sepika_job::settings::AnalysisSettings {
                mass_method,
                ..Default::default()
            };
            close(
                load_total(&auto_loads::compute_dl_beam_loads(&model).unwrap()),
                78500.0,
            );
            close(
                load_total(&sepika_load::slab_load::physical_self_weight_loads(&model).unwrap()),
                76982.2025,
            );
            for _ in 0..2 {
                prepare_model(&mut model, &settings, None, true).unwrap();
                close(
                    model.stories.iter().filter_map(|s| s.seismic_weight).sum(),
                    78500.0,
                );
                close(physical_weight(&model), 76982.2025);
                close(
                    model
                        .layers()
                        .iter()
                        .filter_map(|l| l.dynamic_mass)
                        .map(|m| m.mass_equiv_weight_n / 9806.65)
                        .sum(),
                    7.85,
                );
                assert!(weights_are_current(&model, mass_method));
                println!(
                    "attached={attached}, method={mass_method:?}, physical={} N, mass={} t",
                    physical_weight(&model),
                    physical_weight(&model) / 9806.65
                );
            }
            let cases = sepika_job::gravity_case_ids_for_seismic_weight(&model);
            let mut repeated_cases = cases.clone();
            repeated_cases.extend(&cases);
            let repeated = sepika_load::story_gen::generate_stories_with_synced_self_weight(
                &model,
                &repeated_cases,
                mass_method,
            )
            .unwrap();
            close(
                repeated
                    .stories
                    .iter()
                    .filter_map(|s| s.dynamic_mass)
                    .map(|m| m.mass_equiv_weight_n)
                    .sum(),
                76982.2025,
            );
            let cases_only = sepika_load::story_gen::generate_stories_with_opts(
                &model,
                &cases,
                false,
                mass_method,
            )
            .unwrap();
            close(
                cases_only
                    .stories
                    .iter()
                    .filter_map(|s| s.dynamic_mass)
                    .map(|m| m.mass_equiv_weight_n)
                    .sum(),
                78500.0,
            );
        }
    }
}

#[test]
fn density_derived_physical_distribution_is_independent_of_design_adoption() {
    for attached in [false, true] {
        let mut model = fixture::one_cubic_metre(attached);
        model.materials[1].density = 85e-6 / 9806.65;
        close(
            load_total(&sepika_load::slab_load::physical_self_weight_loads(&model).unwrap()),
            85000.0,
        );
        assert!(prepare_model(&mut model, &Default::default(), None, true)
            .err()
            .unwrap()
            .to_string()
            .contains("過小評価"));
        for density in [-1.0, f64::NAN, f64::INFINITY] {
            model.materials[1].density = density;
            assert!(sepika_load::slab_load::physical_self_weight_loads(&model).is_err());
        }
    }
}

#[test]
fn finish_live_load_and_shared_material_are_preserved_once() {
    for attached in [false, true] {
        let mut model = fixture::one_cubic_metre(attached);
        model.slabs[0].plate.loads.push(AreaLoad {
            kind: "仕上げ".into(),
            value: 0.001,
        });
        model.slabs[0].plate.usage = Some(SlabUsage::Custom {
            floor: 0.004,
            beam: 0.003,
            frame: 0.002,
            seismic: 0.0005,
        });
        for _ in 0..2 {
            prepare_model(&mut model, &Default::default(), None, true).unwrap();
            close(physical_weight(&model), 91982.2025);
            close(
                model.stories.iter().filter_map(|s| s.seismic_weight).sum(),
                93500.0,
            );
        }
        let mut model = fixture::one_cubic_metre(true);
        let mut shared = model.slabs[0].clone();
        shared.id = SlabId(1);
        if let SlabShape::Attached { extent, .. } = &mut shared.shape {
            *extent = extent.map(|v| -v);
        }
        model.slabs.push(shared);
        prepare_model(&mut model, &Default::default(), None, true).unwrap();
        close(physical_weight(&model), 153964.405);
    }
}

#[test]
fn stb_resolved_material_and_all_material_density_validation_apply_to_physical_mass() {
    let mut model = fixture::one_cubic_metre(true);
    model.materials[1].density = f64::NAN;
    model.stb_strengths.members.push(StbMemberStrength {
        target: StrengthTarget::Slab(SlabId(0)),
        node: NodeId(2),
        node_order: vec![NodeId(2), NodeId(3)],
        concrete: Some("Fc24".into()),
    });
    model.prepare_stb_strength_materials();
    let resolved = model.slab_plate_material(&model.slabs[0]).unwrap().id;
    let density = model.materials[resolved.index()].density;
    prepare_model(&mut model, &Default::default(), None, true).unwrap();
    close(physical_weight(&model), density * 1e9 * 9806.65);
    model.materials[resolved.index()].density = -1e-9;
    assert!(sepika_load::slab_load::physical_self_weight_loads(&model).is_err());
    model.materials[resolved.index()].density = density;
    model.sections[2].thickness = Some(f64::INFINITY);
    assert!(sepika_load::slab_load::physical_self_weight_loads(&model).is_err());
    model.sections[2].thickness = Some(100.0);
    model.stb_strengths.materials.clear();
    assert!(sepika_load::slab_load::physical_self_weight_loads(&model).is_err());
}

#[test]
fn generated_slab_mass_reaches_existing_planar_dynamic_model() {
    use sepika_job::lumped_mass::{build_lumped_mass, LumpedMassBuildInput};
    use sepika_solver::dynamic::lumped_mass::{LumpedStiffnessSource, StickDim};
    use sepika_solver::statics::{analysis::SeismicDir, linear::linear_static_once};
    let mut model = fixture::one_cubic_metre(true);
    prepare_model(&mut model, &Default::default(), None, true).unwrap();
    let ex = model
        .load_cases
        .iter()
        .find(|c| c.name == EX_CASE_NAME)
        .unwrap()
        .id;
    let response = linear_static_once(&model, ex).unwrap();
    let result = build_lumped_mass(LumpedMassBuildInput {
        model: &model,
        dim: StickDim::Planar,
        source: LumpedStiffnessSource::StoryQd,
        dir: SeismicDir::X,
        nonlinear: false,
        secant_ratio: 0.75,
        res_x: Some(&response),
        res_y: None,
        po_x: None,
        po_y: None,
    })
    .unwrap();
    close(result.stories.iter().map(|s| s.mass).sum(), 7.85);
}

#[test]
fn partial_attached_support_preserves_density_volume_total_and_invalid_inputs_fail() {
    let mut model = fixture::one_cubic_metre(true);
    if let SlabShape::Attached {
        anchor: RegionAnchor::Line { span, .. },
        ..
    } = &mut model.slabs[0].shape
    {
        *span = [0.2, 0.8];
    }
    prepare_model(&mut model, &Default::default(), None, true).unwrap();
    close(physical_weight(&model), 46189.3215);
    model.materials[1].category = MaterialCategory::Concrete;
    model.materials[1].density = -1e-9;
    let cases = sepika_job::gravity_case_ids_for_seismic_weight(&model);
    let error = sepika_load::story_gen::generate_stories_with_synced_self_weight(
        &model,
        &cases,
        Default::default(),
    )
    .err()
    .unwrap();
    assert!(
        error.contains("物理密度") && error.contains("負"),
        "{error}"
    );
    let error = prepare_model(&mut model, &Default::default(), None, true)
        .err()
        .expect("不正密度は準備計算を停止する")
        .to_string();
    assert!(
        error.contains("物理密度") && error.contains("負"),
        "{error}"
    );
    assert!(!weights_are_current(&model, Default::default()));
    assert!(sepika_job::weight_preparation::require_current_generated_mass(&model).is_err());
    model.materials[1].density = 7.85e-9;
    if let SlabShape::Attached { extent, .. } = &mut model.slabs[0].shape {
        *extent = [f64::NAN; 2];
    }
    assert!(sepika_load::slab_load::physical_self_weight_loads(&model).is_err());
}

#[test]
fn 梁嵩上げの設計重量と物理質量を両質量方式の層と動的モデルへ一度だけ渡す() {
    use sepika_job::lumped_mass::{build_lumped_mass, LumpedMassBuildInput};
    use sepika_solver::dynamic::lumped_mass::{LumpedStiffnessSource, StickDim};
    use sepika_solver::statics::{analysis::SeismicDir, linear::linear_static_once};
    for mass_method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        let mut model = fixture::one_cubic_metre(true);
        for node in &mut model.nodes {
            if node.coord[0] > 0. {
                node.coord[0] = 6000.;
            }
        }
        model.sections[1] = sepika_core::section_shape::SectionShape::SteelH {
            height: 400.,
            width: 400.,
            web_thick: 10.,
            flange_thick: 15.,
            root_r: Some(0.),
        }
        .to_section(SectionId(1), "嵩上げ梁".into());
        model.sections[1].frame_use = Some(FrameSectionUse::Girder);
        model.sections[1].material = Some(MaterialId(0));
        model.elements[2].vertical_eccentricity = Some(-400.);
        let settings = sepika_job::settings::AnalysisSettings {
            mass_method,
            ..Default::default()
        };
        prepare_model(&mut model, &settings, None, true).unwrap();
        close(
            model.stories.iter().filter_map(|s| s.seismic_weight).sum(),
            113040.,
        );
        close(physical_weight(&model), 110854.3716);
        close(
            model
                .layers()
                .iter()
                .filter_map(|l| l.dynamic_mass)
                .map(|m| m.mass_equiv_weight_n / 9806.65)
                .sum(),
            11.304,
        );
        let ex = model
            .load_cases
            .iter()
            .find(|c| c.name == EX_CASE_NAME)
            .unwrap()
            .id;
        let response = linear_static_once(&model, ex).unwrap();
        let result = build_lumped_mass(LumpedMassBuildInput {
            model: &model,
            dim: StickDim::Planar,
            source: LumpedStiffnessSource::StoryQd,
            dir: SeismicDir::X,
            nonlinear: false,
            secant_ratio: 0.75,
            res_x: Some(&response),
            res_y: None,
            po_x: None,
            po_y: None,
        })
        .unwrap();
        close(result.stories.iter().map(|s| s.mass).sum(), 11.304);
    }
}

#[test]
fn attached_tip_gravity_load_is_not_replaced_with_slab_density() {
    let mut model = fixture::one_cubic_metre(true);
    for coord in [[0.0, 2000.0, 12500.0], [5000.0, 2000.0, 12500.0]] {
        let id = NodeId(model.nodes.len() as u32);
        let mut node = model.nodes[2].clone();
        node.id = id;
        node.coord = coord;
        model.nodes.push(node);
    }
    let mut support = model
        .elements
        .iter()
        .find(|e| e.nodes.as_slice() == [NodeId(2), NodeId(3)])
        .unwrap()
        .clone();
    support.id = ElemId(model.elements.len() as u32);
    support.nodes = vec![NodeId(4), NodeId(5)].into();
    model.elements.push(support);
    model.load_cases.push(LoadCase {
        id: LoadCaseId(0),
        name: DL_CASE_NAME.into(),
        kind: LoadCaseKind::Dead,
        nodal: Vec::new(),
        member: Vec::new(),
    });
    model.slabs[0].tip_loads.push(SlabTipLoad {
        case: LoadCaseId(0),
        intensity: 2.0,
        direction: TipLoadDirection::NegZ,
    });
    prepare_model(&mut model, &Default::default(), None, true).unwrap();
    close(physical_weight(&model), 86982.2025);
    close(
        model.stories.iter().filter_map(|s| s.seismic_weight).sum(),
        88500.0,
    );
}
