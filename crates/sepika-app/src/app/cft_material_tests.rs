use super::*;
use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::{model::MaterialCategory, section_shape::SectionShape};
use sepika_edit::{SectionMaterialRole, SetSectionMaterial};

fn set_steel(app: &mut App, material: Option<MaterialId>) -> bool {
    app.apply_model_edit(Box::new(SetSectionMaterial {
        section: SectionId(0),
        role: SectionMaterialRole::Steel,
        material,
    }))
}

fn calculate(app: &mut App) {
    app.run_preparation();
    let index = app.core.model.ultimate_gravity_combination.unwrap();
    let terms = app.core.model.combinations[index].terms.clone();
    for (id, factor) in terms.into_iter().rev() {
        if factor != 0.0 {
            app.run_linear_static(id);
        }
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
    }
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(!app.core.scoped.staleness.design_stale);
    assert!(!app.compute_cft_ultimate_checks().unwrap().is_empty());
}

#[test]
fn cft766_material_edit_save_reopen_invalidation_and_recalculation() {
    for pipe in [false, true] {
        let mut app = super::tests::cft_long_case2_app(1_000_000.);
        if pipe {
            let old = app.core.model.sections[0].clone();
            app.core.model.sections[0] = sepika_core::model::Section {
                material: old.material,
                steel_material: old.steel_material,
                frame_use: old.frame_use,
                ..SectionShape::CftPipe {
                    outer_dia: 400.,
                    thick: 12.,
                }
                .to_section(SectionId(0), "CFT円形".into())
            };
        }
        let mut replacement = app.core.model.materials[0].clone();
        replacement.id = MaterialId(2);
        replacement.name = "独自鋼管345".into();
        replacement.fy = Some(345.);
        replacement.density *= 0.5;
        app.core.model.materials.push(replacement);
        for category in [MaterialCategory::Concrete, MaterialCategory::Rebar] {
            let mut invalid = app.core.model.materials[2].clone();
            invalid.id = MaterialId(app.core.model.materials.len() as u32);
            invalid.category = category;
            app.core.model.materials.push(invalid);
        }
        calculate(&mut app);
        let original = app
            .core
            .model
            .element_mass_properties(&app.core.model.elements[0])
            .unwrap();
        let capacity = app.compute_cft_ultimate_checks().unwrap()[0].ncu;
        let design_weight: f64 = app
            .core
            .model
            .stories
            .iter()
            .filter_map(|s| s.seismic_weight)
            .sum();
        assert!(design_weight > 0.);
        assert!(sepika_job::weight_preparation::weights_are_current(
            &app.core.model,
            app.core.analysis_cfg.mass_method
        ));
        for material in [MaterialId(3), MaterialId(4)] {
            assert!(!set_steel(&mut app, Some(material)));
            assert_eq!(
                app.core.model.sections[0].steel_material,
                Some(MaterialId(0))
            );
            assert!(!app.core.scoped.staleness.design_stale);
        }
        assert!(set_steel(&mut app, Some(MaterialId(2))));
        assert!(app.core.scoped.staleness.preparation_stale);
        assert!(app.core.scoped.staleness.results_stale);
        assert!(app.core.scoped.staleness.design_stale);
        let changed = app
            .core
            .model
            .element_mass_properties(&app.core.model.elements[0])
            .unwrap();
        let steel_area_mm2 = if pipe {
            std::f64::consts::PI * 4656.
        } else {
            18624.
        };
        assert!(
            (original.mass_per_length - changed.mass_per_length - steel_area_mm2 * 3.925e-9).abs()
                < 1e-12
        );
        assert!(!sepika_job::weight_preparation::weights_are_current(
            &app.core.model,
            app.core.analysis_cfg.mass_method
        ));
        app.undo_action();
        assert_eq!(
            app.core.model.sections[0].steel_material,
            Some(MaterialId(0))
        );
        assert_eq!(
            app.core
                .model
                .element_mass_properties(&app.core.model.elements[0])
                .unwrap(),
            original
        );
        app.redo_action();
        assert_eq!(
            app.core.model.sections[0].steel_material,
            Some(MaterialId(2))
        );
        let path =
            std::env::temp_dir().join(format!("sepika766-{pipe}-{}.ovika", std::process::id()));
        app.save_project_to(path.clone());
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert_eq!(
            loaded.core.model.sections[0].steel_material,
            Some(MaterialId(2))
        );
        assert_eq!(loaded.core.model.sections[0].material, Some(MaterialId(1)));
        calculate(&mut loaded);
        assert!(sepika_job::weight_preparation::weights_are_current(
            &loaded.core.model,
            loaded.core.analysis_cfg.mass_method
        ));
        let new_design_weight: f64 = loaded
            .core
            .model
            .stories
            .iter()
            .filter_map(|s| s.seismic_weight)
            .sum();
        assert!((new_design_weight - design_weight).abs() < 1e-6);
        assert!(loaded.compute_cft_ultimate_checks().unwrap()[0].ncu > capacity);
        assert_eq!(
            loaded
                .core
                .model
                .element_mass_properties(&loaded.core.model.elements[0])
                .unwrap(),
            changed
        );
        assert!(set_steel(&mut loaded, None));
        assert!(loaded.core.scoped.staleness.design_stale);
        assert!(loaded.core.scoped.staleness.preparation_stale);
        assert!(loaded.core.scoped.staleness.results_stale);
        assert!(loaded
            .core
            .model
            .element_mass_properties(&loaded.core.model.elements[0])
            .is_err());
        assert!(loaded.compute_cft_ultimate_checks().is_err());
        loaded.save_project_to(path.clone());
        let mut unassigned = App::default();
        unassigned.open_project_from(path.clone());
        assert_eq!(unassigned.core.model.sections[0].steel_material, None);
        unassigned.run_preparation();
        let error = unassigned.core.scoped.last_error.as_ref().unwrap();
        assert!(error.contains("鋼管"), "{error}");
        unassigned.run_design_check();
        assert!(unassigned.core.scoped.last_error.is_some());
        loaded.undo_action();
        calculate(&mut loaded);
        loaded.redo_action();
        assert_eq!(loaded.core.model.sections[0].steel_material, None);
        std::fs::remove_file(path).unwrap();
    }
}
