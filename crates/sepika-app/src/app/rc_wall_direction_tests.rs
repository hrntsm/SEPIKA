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
    WallElement::directional_shear_capacity_at_reference(
        wall,
        &expanded,
        StrengthBasis::MaterialStrength,
        0.0,
        [1.0; 2],
    )
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
    assert_ne!(quantity_before.rebar_t, quantity_after.rebar_t);
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
    assert!(WallElement::directional_shear_capacity_at_reference(
        wall,
        &expanded,
        StrengthBasis::MaterialStrength,
        0.0,
        [1.0; 2]
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
                WallElement::directional_shear_capacity_at_reference(
                    elem,
                    &expanded,
                    StrengthBasis::Nominal,
                    0.0,
                    [1.0; 2],
                )
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
        assert!(WallElement::directional_shear_capacity_at_reference(
            wall,
            &expanded,
            StrengthBasis::Nominal,
            0.0,
            [1.0; 2]
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
    assert!(WallElement::directional_shear_capacity_at_reference(
        wall,
        &expanded,
        StrengthBasis::MaterialStrength,
        0.0,
        [1.0; 2]
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
        None,
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

fn quantity_fixture() -> Model {
    let mut model = fixture();
    for node in &mut model.nodes {
        if node.coord[0] > 0.0 {
            node.coord[0] = 4600.0;
        }
        if node.coord[2] > 0.0 {
            node.coord[2] = 3600.0;
        }
    }
    model.load_cases[0].member = vec![sepika_core::model::MemberLoad::full_length_uniform(
        ElemId(2),
        [0.0, 0.0, -1.0],
        4600.0,
        10.0,
    )];
    model
}
fn wall_quantity(model: &Model) -> sepika_design_jp::quantity::MemberQuantity {
    sepika_design_jp::quantity::try_compute_quantity_takeoff(model, &model.quantity_cfg)
        .unwrap()
        .items
        .into_iter()
        .find(|it| it.category == sepika_design_jp::quantity::MemberCategory::Wall)
        .unwrap()
}
fn quantity_panel_text(app: &mut App) -> String {
    let context = egui::Context::default();
    let mut rendered = String::new();
    for _ in 0..3 {
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(4000.0, 4000.0),
                )),
                ..Default::default()
            },
            |ui| crate::quantity_view::quantity_panel(ui, app),
        );
        fn text(shape: &egui::Shape, out: &mut String) {
            match shape {
                egui::Shape::Text(t) => {
                    out.push_str(t.galley.text());
                    out.push('\n');
                }
                egui::Shape::Vec(parts) => {
                    for part in parts {
                        text(part, out);
                    }
                }
                _ => {}
            }
        }
        for shape in output.shapes {
            text(&shape.shape, &mut rendered);
        }
    }
    rendered
}

#[test]
fn directional_wall_quantity_model_edit_csv_gui_and_ovika_keep_independent_values_and_missing_state(
) {
    use sepika_design_jp::quantity::{QuantityStatus, RebarUsage};
    let mut app = App::default();
    app.load_model(quantity_fixture());
    let before = wall_quantity(&app.core.model);
    assert!((before.rebar[0].total_length_m - 215.432130969).abs() < 1e-9);
    assert!((before.rebar[1].total_length_m - 75.3757810483).abs() < 1e-9);
    let csv = crate::summary::build_quantity_csv(&app.core.model);
    assert!(csv.contains("壁横筋,10,215.432131,0.120642,算定済み概算"));
    assert!(csv.contains("壁縦筋,10,75.375781,0.042210,算定済み概算"));
    let csv_path =
        std::env::temp_dir().join(format!("sepika536-quantity-{}.csv", std::process::id()));
    std::fs::write(&csv_path, &csv).unwrap();
    assert_eq!(std::fs::read_to_string(&csv_path).unwrap(), csv);
    std::fs::remove_file(csv_path).unwrap();
    let panel = quantity_panel_text(&mut app);
    assert!(
        panel.contains("壁横筋: 215.432131 m / 0.120642 t"),
        "{panel}"
    );
    assert!(panel.contains("壁縦筋: 75.375781 m / 0.042210 t"));
    for value in [Some(0.002), None, Some(0.006), None, Some(0.0)] {
        let shape = SectionShape::RcWall {
            thickness: 200.0,
            ps: 0.002,
            pwh_ratio: value,
            pwv_ratio: Some(0.002),
        };
        assert!(
            app.apply_model_edit(Box::new(sepika_edit::EditSectionShape {
                section: SectionId(2),
                new_shape: shape,
                frame_use: None
            }))
        );
        let after = wall_quantity(&app.core.model);
        let vertical = after
            .rebar
            .iter()
            .find(|r| r.usage == RebarUsage::WallVertical)
            .unwrap();
        assert_eq!(vertical.total_length_m, before.rebar[1].total_length_m);
        assert_eq!(vertical.weight_t, before.rebar[1].weight_t);
        if value.is_none() {
            assert_eq!(after.rebar_status(), QuantityStatus::Incomplete);
            assert_eq!(after.rebar_issues[0].wall_plate, Some(WallPlateId(0)));
            assert_eq!(after.rebar_issues[0].section, Some(SectionId(2)));
            let csv = crate::summary::build_quantity_csv(&app.core.model);
            assert!(
                csv.contains("不完全小計")
                    && csv.contains("壁横筋")
                    && csv.contains("方向筋比が未入力")
            );
            for grouping in [
                crate::quantity_view::QuantityGrouping::ByCategory,
                crate::quantity_view::QuantityGrouping::ByStory,
                crate::quantity_view::QuantityGrouping::ByStructure,
                crate::quantity_view::QuantityGrouping::Detail,
                crate::quantity_view::QuantityGrouping::RebarByDia,
            ] {
                app.ui.view.quantity_view.grouping = grouping;
                let text = quantity_panel_text(&mut app);
                assert!(
                    text.contains("不完全小計")
                        && text.contains("壁横筋")
                        && text.contains("方向筋比が未入力"),
                    "{text}"
                );
            }
        } else {
            assert_eq!(after.rebar_status(), QuantityStatus::Complete);
        }
    }
    app.undo_action();
    assert_eq!(
        wall_quantity(&app.core.model).rebar_status(),
        QuantityStatus::Incomplete
    );
    app.redo_action();
    let zero = wall_quantity(&app.core.model);
    assert_eq!(zero.rebar_status(), QuantityStatus::Complete);
    assert_eq!(zero.rebar[0].weight_t, 0.0);
    let path =
        std::env::temp_dir().join(format!("sepika536-direction-{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        wall_quantity(&loaded.core.model).rebar[1].weight_t,
        before.rebar[1].weight_t
    );
    assert_eq!(wall_quantity(&loaded.core.model).rebar[0].weight_t, 0.0);
}

#[test]
fn quantity_setting_edit_undo_redo_save_preserves_rc_mass_response_and_current_result_inputs() {
    let mut app = App::default();
    app.load_model(quantity_fixture());
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(app.require_selected_static_input().is_ok());
    let job_input = app.job_input();
    let weight_input = sepika_job::weight_preparation::weight_input_key(
        &app.core.model,
        app.core.model.mass_method,
    );
    let weight = app
        .core
        .model
        .wall_weight(&app.core.model.wall_plates[0])
        .unwrap()
        .totals;
    let (expanded, _, _) = sepika_load::wall_expand::expand_wall_elements(&app.core.model);
    let elem = expanded
        .elements
        .iter()
        .find(|e| e.kind == ElementKind::Wall)
        .unwrap();
    let mass = sepika_element::factory::build_behavior(elem, &expanded)
        .mass_matrix(sepika_element::behavior::MassOption::Lumped);
    let cfg = sepika_core::model::QuantityCfg {
        assumed_wall_bar_dia: 13.0,
        anchorage_dia_factor: 40.0,
        ..Default::default()
    };
    assert!(
        app.apply_model_edit(Box::new(sepika_edit::CompositeCommand {
            label: "数量仮定変更".into(),
            children: vec![Box::new(sepika_edit::SetQuantityCfg { cfg })]
        }))
    );
    assert!(app.core.scoped.staleness.unsaved_changes);
    assert!(!app.core.scoped.staleness.results_stale);
    assert!(!app.core.scoped.staleness.preparation_stale);
    assert!(app.require_selected_static_input().is_ok());
    assert_eq!(app.job_input(), job_input);
    assert_eq!(
        sepika_job::weight_preparation::weight_input_key(
            &app.core.model,
            app.core.model.mass_method
        ),
        weight_input
    );
    let changed = wall_quantity(&app.core.model);
    assert!((changed.rebar[0].total_length_m - 136.696202956656).abs() < 1e-9);
    assert!((changed.rebar[1].weight_t - 0.0484560314501908).abs() < 1e-12);
    assert_eq!(
        app.core
            .model
            .wall_weight(&app.core.model.wall_plates[0])
            .unwrap()
            .totals,
        weight
    );
    let (expanded, _, _) = sepika_load::wall_expand::expand_wall_elements(&app.core.model);
    let elem = expanded
        .elements
        .iter()
        .find(|e| e.kind == ElementKind::Wall)
        .unwrap();
    let after = sepika_element::factory::build_behavior(elem, &expanded)
        .mass_matrix(sepika_element::behavior::MassOption::Lumped);
    for i in 0..24 {
        for j in 0..24 {
            assert_eq!(mass.get(i, j), after.get(i, j));
        }
    }
    app.undo_action();
    assert!(app.require_selected_static_input().is_ok());
    assert!(!app.core.scoped.staleness.results_stale);
    assert!((wall_quantity(&app.core.model).rebar[0].weight_t - 0.120641993343).abs() < 1e-12);
    app.redo_action();
    assert!(app.require_selected_static_input().is_ok());
    assert!(!app.core.scoped.staleness.results_stale);
    assert_eq!(app.core.model.quantity_cfg, cfg);
    let response = app.core.scoped.results.as_ref().unwrap().statics[0]
        .1
        .disp
        .clone();
    assert!(response.iter().flatten().any(|v| v.abs() > 0.0));
    app.run_linear_static(LoadCaseId(0));
    assert_eq!(
        app.core.scoped.results.as_ref().unwrap().statics[0].1.disp,
        response
    );
    let path =
        std::env::temp_dir().join(format!("sepika536-settings-{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();
    assert_eq!(loaded.core.model.quantity_cfg, cfg);
    assert_eq!(
        wall_quantity(&loaded.core.model).rebar[0].weight_t,
        changed.rebar[0].weight_t
    );
    assert!(loaded.require_selected_static_input().is_ok());
}

#[test]
fn current_schema_ps_only_model_keeps_missing_directions_after_ovika_read() {
    let mut value = serde_json::to_value(quantity_fixture()).unwrap();
    let wall = value["sections"][2]["shape"]["RcWall"]
        .as_object_mut()
        .unwrap();
    wall.remove("pwh_ratio");
    wall.remove("pwv_ratio");
    let model: Model = serde_json::from_value(value).unwrap();
    let path = std::env::temp_dir().join(format!("sepika536-ps-only-{}.ovika", std::process::id()));
    sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
    let loaded = sepika_io::ovika::load_ovika(&path).unwrap().model;
    std::fs::remove_file(path).unwrap();
    let w = wall_quantity(&loaded);
    assert!(w.rebar.is_empty());
    assert_eq!(w.rebar_issues.len(), 2);
    assert_eq!(
        w.rebar_status(),
        sepika_design_jp::quantity::QuantityStatus::Incomplete
    );
}
