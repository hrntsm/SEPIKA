use super::*;
use sepika_core::ids::WallPlateId;
use sepika_core::model::{
    ElementKind, MaterialCategory, Model, StbGradeMaterial, StbMemberStrength, StrengthTarget,
};
use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};
use sepika_element::factory::StrengthBasis;
use sepika_element::wall::wall_element::WallElement;

fn fixture() -> Model {
    let mut model = super::steel_wall_notice_tests::steel_wall_model();
    model.panel_zone = sepika_core::model::PanelZoneMode::None;
    model.materials.truncate(1);
    let concrete = &mut model.materials[0];
    concrete.name = "Fc24".into();
    concrete.category = MaterialCategory::Concrete;
    concrete.fc = Some(24.0);
    concrete.fy = None;
    concrete.young = 23000.0;
    concrete.poisson = 0.2;
    concrete.density = 2.4e-9;
    concrete.shear = None;
    let mut vertical = concrete.clone();
    vertical.id = MaterialId(1);
    vertical.name = "SD345".into();
    vertical.category = MaterialCategory::Rebar;
    vertical.fc = None;
    vertical.fy = Some(345.0);
    vertical.young = 205000.0;
    vertical.strength_factor = Some(1.0);
    let mut horizontal = vertical.clone();
    horizontal.id = MaterialId(2);
    horizontal.name = "SD295A".into();
    horizontal.fy = Some(295.0);
    model.materials.extend([vertical, horizontal]);
    let frame = SectionShape::RcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: 22.0,
            x: vec![8],
            y: vec![8],
            cover: 50.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
    };
    for index in 0..3 {
        let old = &model.sections[index];
        let shape = if index == 2 {
            SectionShape::RcWall {
                thickness: 200.0,
                ps: 0.0025,
                pwh_ratio: Some(0.006),
                pwv_ratio: Some(0.002),
            }
        } else {
            frame.clone()
        };
        let mut section = shape.to_section(old.id, old.name.clone());
        section.frame_use = old.frame_use;
        section.material = Some(MaterialId(0));
        section.rebar_material = Some(MaterialId(1));
        section.shear_rebar_material = Some(MaterialId(2));
        model.sections[index] = section;
    }
    for element in &mut model.elements {
        element.force_regime = sepika_core::model::ForceRegime::AxialBendingInteract;
    }
    model
}

fn capacity(model: &Model) -> [f64; 2] {
    let (expanded, _, _) = sepika_load::wall_expand::expand_wall_elements(model);
    let wall = expanded
        .elements
        .iter()
        .find(|e| e.kind == ElementKind::Wall)
        .unwrap();
    WallElement::directional_shear_capacity_of(wall, &expanded, StrengthBasis::MaterialStrength)
        .unwrap()
}

#[test]
fn gui_direction_edit_save_undo_and_material_edit_invalidate_previous_analysis() {
    let mut app = App::default();
    app.load_model(fixture());
    app.core.analysis_cfg.push_apply_long_term = false;
    app.core.analysis_cfg.push_steps = 3;
    app.core.analysis_cfg.push_max_disp = 1.0;
    app.core.analysis_cfg.push_use_max_disp = true;
    app.core.analysis_cfg.push_use_drift_angle = false;
    app.run_preparation();
    app.run_pushover();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(!app.displayed_pushover().unwrap().steps.is_empty());
    let before = app.core.model.sections[2].shape.clone().unwrap();
    let qu = capacity(&app.core.model);
    let quantity_before = sepika_design_jp::quantity::try_compute_quantity_takeoff(
        &app.core.model,
        &Default::default(),
    )
    .unwrap()
    .totals();
    let weight = app
        .core
        .model
        .wall_weight(&app.core.model.wall_plates[0])
        .unwrap()
        .totals;
    let edited = SectionShape::RcWall {
        thickness: 200.0,
        ps: 0.0025,
        pwh_ratio: Some(0.002),
        pwv_ratio: Some(0.006),
    };
    assert!(
        app.apply_model_edit(Box::new(sepika_edit::EditSectionShape {
            section: SectionId(2),
            new_shape: edited.clone(),
            frame_use: None
        }))
    );
    assert!(app.core.scoped.staleness.results_stale && app.core.scoped.staleness.preparation_stale);
    assert!(capacity(&app.core.model)[0] < qu[0]);
    assert_eq!(
        app.core
            .model
            .wall_weight(&app.core.model.wall_plates[0])
            .unwrap()
            .totals,
        weight
    );
    let quantity_after = sepika_design_jp::quantity::try_compute_quantity_takeoff(
        &app.core.model,
        &Default::default(),
    )
    .unwrap()
    .totals();
    assert_eq!(quantity_before.concrete_m3, quantity_after.concrete_m3);
    assert_eq!(quantity_before.rebar_t, quantity_after.rebar_t);
    assert_eq!(quantity_before.formwork_m2, quantity_after.formwork_m2);
    app.undo_action();
    assert_eq!(app.core.model.sections[2].shape.as_ref(), Some(&before));
    assert_eq!(capacity(&app.core.model), qu);
    app.redo_action();
    assert_eq!(app.core.model.sections[2].shape.as_ref(), Some(&edited));
    let path = std::env::temp_dir().join(format!(
        "sepika535-direction-gui-{}.ovika",
        std::process::id()
    ));
    app.save_project_to(path.clone());
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();
    assert_eq!(loaded.core.model.sections[2].shape.as_ref(), Some(&edited));
    loaded.run_preparation();
    loaded.run_pushover();
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
    let before = capacity(&loaded.core.model);
    assert!(
        loaded.apply_model_edit(Box::new(sepika_edit::SetMaterialField {
            id: MaterialId(2),
            field: sepika_edit::MaterialField::Fy,
            value: Some(345.0)
        }))
    );
    assert!(
        loaded.core.scoped.staleness.results_stale
            && loaded.core.scoped.staleness.preparation_stale
    );
    assert!(capacity(&loaded.core.model)[0] > before[0]);
    assert!(loaded.apply_model_edit(Box::new(sepika_edit::AddMaterial {
        name: "SD295A".into(),
        category: MaterialCategory::Rebar,
        young: 205000.0,
        poisson: 0.3,
        density: 7.85e-9,
        fc: None,
        fy: Some(295.0),
        strength_factor: Some(1.0),
        concrete_class: Default::default(),
    })));
    let replacement = MaterialId(loaded.core.model.materials.len() as u32 - 1);
    assert!(
        loaded.apply_model_edit(Box::new(sepika_edit::SetSectionMaterial {
            section: SectionId(2),
            role: sepika_edit::SectionMaterialRole::ShearRebar,
            material: Some(replacement),
        }))
    );
    loaded.run_preparation();
    loaded.run_pushover();
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
    assert!(
        loaded.apply_model_edit(Box::new(sepika_edit::SetSectionMaterial {
            section: SectionId(2),
            role: sepika_edit::SectionMaterialRole::ShearRebar,
            material: None,
        }))
    );
    assert!(
        loaded.core.scoped.staleness.results_stale
            && loaded.core.scoped.staleness.preparation_stale
    );
    loaded.run_pushover();
    assert!(loaded
        .core
        .scoped
        .last_error
        .as_ref()
        .unwrap()
        .contains("横筋材料"));
    assert!(loaded.apply_model_edit(Box::new(sepika_edit::DeleteMaterial { id: replacement })));
    loaded.undo_action();
    assert!(
        loaded.apply_model_edit(Box::new(sepika_edit::SetSectionMaterial {
            section: SectionId(2),
            role: sepika_edit::SectionMaterialRole::ShearRebar,
            material: Some(replacement),
        }))
    );
    loaded.run_preparation();
    loaded.run_pushover();
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
}

#[test]
fn preparation_and_pushover_report_the_same_missing_wall_direction() {
    let mut model = fixture();
    if let Some(SectionShape::RcWall { pwv_ratio, .. }) = &mut model.sections[2].shape {
        *pwv_ratio = None;
    }
    let mut app = App::default();
    app.load_model(model);
    app.run_preparation();
    let (expanded, _, _) = sepika_load::wall_expand::expand_wall_elements(&app.core.model);
    let wall = expanded
        .elements
        .iter()
        .find(|e| e.kind == ElementKind::Wall)
        .unwrap();
    assert!(WallElement::directional_shear_capacity_of(
        wall,
        &expanded,
        StrengthBasis::MaterialStrength
    )
    .is_ok());
    let reason =
        WallElement::wall_shear_capacity_issue(wall, &expanded, StrengthBasis::MaterialStrength)
            .unwrap();
    assert!(reason.contains("縦筋比"));
    assert!(app
        .core
        .log
        .entries
        .iter()
        .any(|entry| entry.message.contains(&reason)));
    app.run_pushover();
    assert!(app
        .core
        .scoped
        .last_error
        .as_ref()
        .is_some_and(|e| e.contains(&reason)));
    assert!(app.displayed_pushover().is_none());
}

#[test]
fn same_section_wall_member_fc_and_direction_materials_survive_expansion_and_renumbering() {
    let mut model = fixture();
    let original_nodes = model.nodes.clone();
    let node_offset = original_nodes.len() as u32;
    for mut node in original_nodes {
        node.id.0 += node_offset;
        node.coord[0] += 8000.0;
        model.nodes.push(node);
    }
    let original_elements = model.elements.clone();
    for mut element in original_elements {
        element.id = ElemId(model.elements.len() as u32);
        for node in &mut element.nodes {
            node.0 += node_offset;
        }
        model.elements.push(element);
    }
    let plate = model.wall_plates[0].clone();
    let second = model.add_enclosed_wall_plate_from_nodes(
        &[
            NodeId(node_offset),
            NodeId(node_offset + 1),
            NodeId(node_offset + 3),
            NodeId(node_offset + 2),
        ],
        plate,
    );
    let mut fc30 = model.materials[0].clone();
    fc30.id = MaterialId(3);
    fc30.name = "Fc30".into();
    fc30.fc = Some(30.0);
    model.materials.push(fc30);
    model.stb_strengths.materials = vec![
        StbGradeMaterial {
            grade: "Fc24".into(),
            material: MaterialId(0),
        },
        StbGradeMaterial {
            grade: "Fc30".into(),
            material: MaterialId(3),
        },
    ];
    for (wall, node, grade) in [
        (WallPlateId(0), NodeId(0), "Fc30"),
        (second, NodeId(node_offset), "Fc24"),
    ] {
        model.stb_strengths.members.push(StbMemberStrength {
            target: StrengthTarget::Wall(wall),
            node,
            node_order: vec![node],
            concrete: Some(grade.into()),
        });
    }
    sepika_core::wall_region_rebuild::rebuild_wall_regions(&mut model);
    for reverse in [false, true] {
        if reverse {
            let count = model.elements.len() as u32;
            model.visit_elem_ids(|id| id.0 = count - 1 - id.0);
            model.elements.reverse();
        }
        let (expanded, _, _) = sepika_load::wall_expand::expand_wall_elements(&model);
        let mut capacities = Vec::new();
        for (plate, fc) in [(WallPlateId(0), 30.0), (second, 24.0)] {
            let elem = expanded
                .generated_wall_origins
                .iter()
                .find(|(_, origin)| **origin == plate)
                .map(|(id, _)| expanded.element(*id).unwrap())
                .unwrap();
            assert_eq!(expanded.element_material(elem).unwrap().fc, Some(fc));
            assert_eq!(
                expanded.element_rebar_material(elem).unwrap().fy,
                Some(345.0)
            );
            assert_eq!(
                expanded.element_shear_rebar_material(elem).unwrap().fy,
                Some(295.0)
            );
            capacities.push(
                WallElement::directional_shear_capacity_of(elem, &expanded, StrengthBasis::Nominal)
                    .unwrap(),
            );
        }
        assert!(capacities[0][0] > capacities[1][0]);
    }
}

#[test]
fn wall_stb_parts_and_positions_require_explicit_direction_roles() {
    use sepika_core::model::{StbRebarStrength, StbSectionStrength};
    let mut model = fixture();
    let bar = |part: &str, position: Option<&str>, native_material| StbRebarStrength {
        element: "StbSecBarWall".into(),
        part: part.into(),
        position: position.map(str::to_owned),
        native_material,
        diameter: None,
        strength: None,
    };
    model.stb_strengths.sections.push(StbSectionStrength {
        section: SectionId(2),
        concrete: None,
        native_material: None,
        native_rebar: false,
        rebar: None,
        reinforcement: vec![
            bar("main", None, Some(MaterialId(1))),
            bar("stirrup", None, Some(MaterialId(2))),
        ],
        steel: vec![],
    });
    model.sections[2].rebar_material = None;
    model.sections[2].shear_rebar_material = None;
    let (expanded, _, _) = sepika_load::wall_expand::expand_wall_elements(&model);
    let wall = expanded
        .elements
        .iter()
        .find(|e| e.kind == ElementKind::Wall)
        .unwrap();
    assert!(expanded.element_rebar_material(wall).is_none());
    assert!(expanded.element_shear_rebar_material(wall).is_none());
    model.stb_strengths.sections[0].reinforcement = vec![
        bar("vertical", None, Some(MaterialId(1))),
        bar("horizontal", None, Some(MaterialId(2))),
    ];
    assert!(capacity(&model)[0] > 0.0);
    for position in [Some("TOP"), Some("BOTTOM")] {
        model.stb_strengths.sections[0].reinforcement[1].position = position.map(str::to_owned);
        let (expanded, _, _) = sepika_load::wall_expand::expand_wall_elements(&model);
        let wall = expanded
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Wall)
            .unwrap();
        assert!(WallElement::directional_shear_capacity_of(
            wall,
            &expanded,
            StrengthBasis::Nominal
        )
        .unwrap_err()
        .contains("横筋材料"));
    }
    model.stb_strengths.sections[0].reinforcement[1].position = None;
    let mut same_grade = model.materials[2].clone();
    same_grade.id = MaterialId(3);
    same_grade.strength_factor = Some(1.2);
    model.materials.push(same_grade);
    model.stb_strengths.sections[0].reinforcement.push(bar(
        "horizontal",
        None,
        Some(MaterialId(3)),
    ));
    let (expanded, _, _) = sepika_load::wall_expand::expand_wall_elements(&model);
    let wall = expanded
        .elements
        .iter()
        .find(|e| e.kind == ElementKind::Wall)
        .unwrap();
    assert!(WallElement::directional_shear_capacity_of(
        wall,
        &expanded,
        StrengthBasis::MaterialStrength
    )
    .unwrap_err()
    .contains("横筋材料"));
    let mut app = App::default();
    app.load_model(model);
    assert!(
        app.apply_model_edit(Box::new(sepika_edit::SetSectionMaterial {
            section: SectionId(2),
            role: sepika_edit::SectionMaterialRole::ShearRebar,
            material: Some(MaterialId(2)),
        }))
    );
    assert!(capacity(&app.core.model)[0] > 0.0);
    assert!(
        app.apply_model_edit(Box::new(sepika_edit::SetSectionMaterial {
            section: SectionId(2),
            role: sepika_edit::SectionMaterialRole::ShearRebar,
            material: None,
        }))
    );
    let (expanded, _, _) = sepika_load::wall_expand::expand_wall_elements(&app.core.model);
    let wall = expanded
        .elements
        .iter()
        .find(|e| e.kind == ElementKind::Wall)
        .unwrap();
    assert!(expanded.element_shear_rebar_material(wall).is_none());
    assert_eq!(
        expanded.element_rebar_material(wall).unwrap().id,
        MaterialId(1)
    );
    app.undo_action();
    assert!(capacity(&app.core.model)[0] > 0.0);
}

#[test]
fn explicitly_nonseismic_rc_wall_keeps_elastic_mode_without_directional_inputs() {
    let mut model = fixture();
    model.wall_plates[0].slit.column_face[0] = true;
    model.sections[2].shape = Some(SectionShape::RcWall {
        thickness: 200.0,
        ps: 0.0025,
        pwh_ratio: None,
        pwv_ratio: None,
    });
    model.sections[2].rebar_material = None;
    model.sections[2].shear_rebar_material = None;
    let mut app = App::default();
    app.load_model(model);
    app.core.analysis_cfg.push_apply_long_term = false;
    app.core.analysis_cfg.push_steps = 3;
    app.core.analysis_cfg.push_max_disp = 1.0;
    app.run_preparation();
    let (expanded, _, _) = sepika_load::wall_expand::expand_wall_elements(&app.core.model);
    let wall = expanded
        .elements
        .iter()
        .find(|e| e.kind == ElementKind::Wall)
        .unwrap();
    assert!(!sepika_element::wall::misc_wall::wall_is_seismic(
        wall, &expanded
    ));
    assert!(WallElement::wall_shear_capacity_issue(
        wall,
        &expanded,
        StrengthBasis::MaterialStrength
    )
    .is_none());
    let linear = sepika_element::factory::build_behavior(wall, &expanded);
    let nonlinear = sepika_element::factory::build_nonlinear_behavior(
        wall,
        &expanded,
        StrengthBasis::MaterialStrength,
        sepika_core::model::AnalysisKind::Incremental,
    );
    let ctx = sepika_element::behavior::Ctx { model: &expanded };
    let expected = linear.tangent_stiffness(&ctx);
    let actual = nonlinear.tangent_stiffness(&ctx);
    for row in 0..24 {
        for col in 0..24 {
            assert_eq!(expected.get(row, col), actual.get(row, col));
        }
    }
    app.run_pushover();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(!app.displayed_pushover().unwrap().steps.is_empty());
}
