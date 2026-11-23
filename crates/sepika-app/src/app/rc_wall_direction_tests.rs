use super::wall_material_tests::rc_wall_model as fixture;
use super::*;
use sepika_core::ids::WallPlateId;
use sepika_core::model::{ElementKind, MaterialCategory, Model};
use sepika_core::section_shape::SectionShape;
use sepika_element::factory::StrengthBasis;
use sepika_element::wall::wall_element::WallElement;

fn capacity(model: &Model) -> [f64; 2] {
    let (expanded, _, _) = sepika_core::wall_expand::expand_wall_elements(model);
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
    let (expanded, _, _) = sepika_core::wall_expand::expand_wall_elements(&app.core.model);
    let wall = expanded
        .elements
        .iter()
        .find(|e| e.kind == ElementKind::Wall)
        .unwrap();
    let capacity_reason = WallElement::directional_shear_capacity_at_reference(
        wall,
        &expanded,
        StrengthBasis::MaterialStrength,
        0.0,
        [1.0; 2],
    )
    .unwrap_err();
    assert!(capacity_reason.contains("縦筋比"));
    let reason =
        WallElement::wall_shear_capacity_issue(wall, &expanded, StrengthBasis::MaterialStrength)
            .unwrap();
    assert_eq!(capacity_reason, reason);
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
fn wall_section_fc_and_direction_materials_survive_expansion_and_renumbering() {
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
    let mut alternate = model
        .wall_plate_section(&model.wall_plates[0])
        .unwrap()
        .clone();
    alternate.id = SectionId(model.sections.len() as u32);
    alternate.material = Some(MaterialId(3));
    model.wall_plates[0].section = Some(alternate.id);
    model.sections.push(alternate);
    sepika_core::wall_region_rebuild::rebuild_wall_regions(&mut model);
    for reverse in [false, true] {
        if reverse {
            let count = model.elements.len() as u32;
            model.visit_elem_ids(|id| id.0 = count - 1 - id.0);
            model.elements.reverse();
        }
        let (expanded, _, _) = sepika_core::wall_expand::expand_wall_elements(&model);
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
    let (expanded, _, _) = sepika_core::wall_expand::expand_wall_elements(&app.core.model);
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
    model.stories = super::tests::explicit_story_inputs(&[0.0, 3600.0]);
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
    let (expanded, _, _) = sepika_core::wall_expand::expand_wall_elements(&app.core.model);
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
    let (expanded, _, _) = sepika_core::wall_expand::expand_wall_elements(&app.core.model);
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

fn quantity_ratio_fixture() -> Model {
    use sepika_core::{
        ids::{NodeId, SlabId, StoryId},
        model::{LoadTransfer, RegionAnchor, Slab, SlabPlate, SlabShape},
    };

    let mut model = quantity_fixture();
    let mut steel = crate::sample::portal_frame().materials.remove(0);
    steel.id = MaterialId(3);
    model.materials.push(steel);
    let rc = &model.sections[1];
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &rc.shape else {
        unreachable!()
    };
    let mut src = SectionShape::SrcBeamRect {
        b: 600.0,
        d: 600.0,
        rebar: rebar.clone(),
        steel_height: 400.0,
        steel_width: 200.0,
        steel_web_thick: 8.0,
        steel_flange_thick: 13.0,
    }
    .to_section(SectionId(3), "SRC小梁".into());
    src.frame_use = rc.frame_use;
    src.material = rc.material;
    src.rebar_material = rc.rebar_material;
    src.shear_rebar_material = rc.shear_rebar_material;
    src.steel_material = Some(MaterialId(3));
    model.sections.push(src);

    for (id, x_mm, y_mm) in [(4, 4600.0, 2000.0), (5, 0.0, 2000.0), (6, 0.0, 4000.0)] {
        let mut node = model.nodes[2].clone();
        node.id = NodeId(id);
        node.coord = [x_mm, y_mm, 3600.0];
        model.nodes.push(node);
    }
    for (id, i, j, section) in [(4, 3, 4, 1), (5, 4, 5, 3), (6, 5, 2, 1), (7, 5, 6, 1)] {
        let mut member = model.elements[2].clone();
        member.id = ElemId(id);
        member.nodes = [NodeId(i), NodeId(j)].into_iter().collect();
        member.section = Some(SectionId(section));
        model.elements.push(member);
    }

    let slab_section = SectionId(4);
    let mut section =
        SectionShape::RcSlab { thickness: 150.0 }.to_section(slab_section, "床断面".into());
    section.material = Some(MaterialId(0));
    model.sections.push(section);
    let plate = SlabPlate {
        section: Some(slab_section),
        ..Default::default()
    };
    let enclosed = model
        .add_enclosed_slab_from_nodes(&[NodeId(2), NodeId(3), NodeId(4), NodeId(5)], plate.clone());
    model.slabs[enclosed.index()].reference_story = Some(StoryId(1));
    model.slabs[enclosed.index()].vertical_eccentricity_mm = Some(300.0);
    model.slabs.push(Slab {
        id: SlabId(1),
        shape: SlabShape::Attached {
            anchor: RegionAnchor::Line {
                // RC梁の床板協力剛性は取り付く床板に未対応なので、SRC梁に取り付ける。
                nodes: [NodeId(4), NodeId(5)],
                span: [0.0, 1.0],
                transfer: LoadTransfer::Anchor,
            },
            extent: [-1000.0; 2],
        },
        plate,
        reference_story: Some(StoryId(1)),
        // 取り付く床板の実重複控除は未対応なので、床下面を梁上端に揃える。
        vertical_eccentricity_mm: Some(450.0),
        tip_loads: Vec::new(),
    });
    model
}

#[test]
fn quantity_ratio_inputs_change_only_their_estimates_and_roundtrip_without_staling_results() {
    use sepika_design_jp::quantity::{
        try_compute_quantity_takeoff, MemberCategory, RebarUsage, StructureKind,
    };

    let mut app = App::default();
    app.load_model(quantity_ratio_fixture());
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let results = app.current_design_results().unwrap();
    assert!(!results.member_checks.is_empty());
    let check_values = |results: &ResultsBundle| {
        rmp_serde::to_vec_named(&(
            &results.member_checks,
            &results.slab_checks,
            &results.wall_checks,
        ))
        .unwrap()
    };
    let checks = check_values(results);
    let static_response = results.statics[0].1.disp.clone();
    let input_keys = [
        ResultInputKey::Static(StaticCaseKey::User(LoadCaseId(0))),
        ResultInputKey::Design,
    ];
    let inputs = input_keys.each_ref().map(|key| app.result_input(key));
    let assert_results = |app: &App| {
        assert!(app.require_selected_static_input().is_ok());
        assert!(!app.core.scoped.staleness.results_stale);
        assert!(!app.core.scoped.staleness.design_stale);
        assert_eq!(
            input_keys.each_ref().map(|key| app.result_input(key)),
            inputs
        );
        let current = app.current_design_results().unwrap();
        assert_eq!(current.statics[0].1.disp, static_response);
        assert_eq!(check_values(current), checks);
    };
    let takeoff = |app: &App| {
        let model = &app.core.model;
        try_compute_quantity_takeoff(model, &model.quantity_cfg).unwrap()
    };

    let initial = takeoff(&app);
    for (category, structure) in [
        (MemberCategory::Beam, StructureKind::Rc),
        (MemberCategory::Beam, StructureKind::Src),
        (MemberCategory::Slab, StructureKind::Rc),
        (MemberCategory::CantileverSlab, StructureKind::Rc),
        (MemberCategory::Girder, StructureKind::Rc),
        (MemberCategory::Wall, StructureKind::Rc),
    ] {
        assert!(initial.items.iter().any(|item| {
            item.category == category && item.structure == structure && item.rebar_weight_t() > 0.0
        }));
    }

    let ctx = egui::Context::default();
    install_japanese_fonts(&ctx);
    app.ui.view.design_view = DesignView::Quantities;
    let frame = |app: &mut App, events| {
        ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show_inside(ui, |ui| app.design_tab_panel(ui));
            },
        )
    };
    let position = |out: &egui::FullOutput, matches_text: &dyn Fn(&str) -> bool| {
        out.shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if matches_text(t.galley.text()) => {
                    Some(t.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("数量設定の文字が描画される")
    };
    let number_position = |out: &egui::FullOutput, value| {
        position(out, &|text| text.parse::<f64>().ok() == Some(value))
    };
    let click = |app: &mut App, pos| {
        for pressed in [true, false] {
            frame(
                app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    };
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    let edit = |app: &mut App, pos, text: &str| {
        click(app, pos);
        let revision = app.core.scoped.undo.revision();
        frame(
            app,
            vec![
                key(egui::Key::A, egui::Modifiers::COMMAND),
                egui::Event::Text(text.into()),
            ],
        );
        assert_eq!(app.core.scoped.undo.revision(), revision);
        frame(app, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    };

    let mut cfg = app.core.model.quantity_cfg;
    for (usage, label, default, after_text, multiplier) in [
        (
            RebarUsage::BeamMain,
            "主架構小梁の主筋比",
            0.008,
            "0.016",
            2.0,
        ),
        (
            RebarUsage::BeamStirrup,
            "主架構小梁のスターラップ比",
            0.001,
            "0.003",
            3.0,
        ),
        (
            RebarUsage::SlabBar,
            "床・片持ち床の鉄筋比",
            0.01,
            "0.04",
            4.0,
        ),
    ] {
        let before = takeoff(&app);
        let previous_cfg = cfg;
        let out = frame(&mut app, vec![]);
        position(&out, &|text| {
            text == "鉄筋の概算に用いるコンクリート体積比 [m³/m³]"
        });
        position(&out, &|text| text == label);
        edit(&mut app, number_position(&out, default), after_text);
        let next = after_text.parse().unwrap();
        match usage {
            RebarUsage::BeamMain => cfg.beam_main_ratio = next,
            RebarUsage::BeamStirrup => cfg.beam_stirrup_ratio = next,
            RebarUsage::SlabBar => cfg.slab_rebar_ratio = next,
            _ => unreachable!(),
        }
        assert_eq!(app.core.model.quantity_cfg, cfg, "{label}");
        let after = takeoff(&app);
        assert_eq!(before.items.len(), after.items.len());
        for (previous, changed) in before.items.iter().zip(&after.items) {
            assert_eq!(previous.label, changed.label);
            assert_eq!(previous.concrete_m3, changed.concrete_m3);
            assert_eq!(previous.formwork_m2, changed.formwork_m2);
            assert_eq!(previous.steel_weight_t(), changed.steel_weight_t());
            assert_eq!(previous.rebar_joints, changed.rebar_joints);
            assert_eq!(previous.rebar.len(), changed.rebar.len());
            for (old, new) in previous.rebar.iter().zip(&changed.rebar) {
                assert_eq!(old.usage, new.usage);
                assert_eq!(old.dia, new.dia);
                assert_eq!(old.total_length_m, new.total_length_m);
                if old.usage == usage {
                    assert!((new.weight_t - multiplier * old.weight_t).abs() < 1e-12);
                } else {
                    assert_eq!(old.weight_t, new.weight_t);
                }
            }
        }
        assert_results(&app);

        app.undo_action();
        assert_eq!(app.core.model.quantity_cfg, previous_cfg);
        assert_eq!(takeoff(&app).totals().rebar_t, before.totals().rebar_t);
        assert_results(&app);
        app.redo_action();
        assert_eq!(app.core.model.quantity_cfg, cfg);
        assert_eq!(takeoff(&app).totals().rebar_t, after.totals().rebar_t);
        assert_results(&app);
    }

    let revision = app.core.scoped.undo.revision();
    for invalid in ["abc", "NaN"] {
        let out = frame(&mut app, vec![]);
        edit(
            &mut app,
            number_position(&out, cfg.beam_main_ratio),
            invalid,
        );
        assert_eq!(app.core.model.quantity_cfg, cfg);
        assert_eq!(app.core.scoped.undo.revision(), revision);
    }
    let out = frame(&mut app, vec![]);
    edit(
        &mut app,
        number_position(&out, cfg.beam_main_ratio),
        "-0.01",
    );
    assert_eq!(app.core.model.quantity_cfg.beam_main_ratio, 0.0);
    let out = frame(&mut app, vec![]);
    click(&mut app, number_position(&out, 0.0));
    let boundary_revision = app.core.scoped.undo.revision();
    frame(
        &mut app,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
    );
    assert_eq!(app.core.model.quantity_cfg.beam_main_ratio, 0.0);
    assert_eq!(app.core.scoped.undo.revision(), boundary_revision);
    frame(
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    app.undo_action();
    assert_eq!(app.core.model.quantity_cfg, cfg);

    let csv = crate::summary::build_quantity_csv(&app.core.model);
    let path =
        std::env::temp_dir().join(format!("sepika-1744-ratios-{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    std::fs::remove_file(&path).unwrap();
    assert!(
        reopened.core.scoped.last_error.is_none(),
        "{:?}",
        reopened.core.scoped.last_error
    );
    assert_eq!(reopened.core.model.quantity_cfg, cfg);
    assert_eq!(
        crate::summary::build_quantity_csv(&reopened.core.model),
        csv
    );
    assert_results(&reopened);

    let legacy_cfg = sepika_core::model::QuantityCfg {
        beam_main_ratio: -0.02,
        slab_rebar_ratio: 0.012345678901234567,
        ..cfg
    };
    reopened.core.model.quantity_cfg = legacy_cfg;
    sepika_io::ovika::save_ovika(&path, &reopened.core.model, Default::default()).unwrap();
    let mut legacy = App::default();
    legacy.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();
    assert_eq!(legacy.core.model.quantity_cfg, legacy_cfg);
    legacy.ui.view.design_view = DesignView::Quantities;
    let revision = legacy.core.scoped.undo.revision();
    for old in [legacy_cfg.beam_main_ratio, legacy_cfg.slab_rebar_ratio] {
        let out = frame(&mut legacy, vec![]);
        click(&mut legacy, number_position(&out, old));
    }
    let out = frame(&mut legacy, vec![]);
    assert_eq!(legacy.core.scoped.undo.revision(), revision);
    edit(
        &mut legacy,
        number_position(&out, legacy_cfg.beam_stirrup_ratio),
        "0.006",
    );
    assert_eq!(
        legacy.core.model.quantity_cfg,
        sepika_core::model::QuantityCfg {
            beam_stirrup_ratio: 0.006,
            ..legacy_cfg
        }
    );
    legacy.undo_action();
    assert_eq!(legacy.core.model.quantity_cfg, legacy_cfg);
}

#[test]
fn saved_quantity_ratios_show_all_invalid_inputs_in_panel_and_csv() {
    let ctx = egui::Context::default();
    install_japanese_fonts(&ctx);
    let frame = |app: &mut App, events| {
        ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show_inside(ui, |ui| app.design_tab_panel(ui));
            },
        )
    };
    let model = quantity_ratio_fixture();
    let path =
        std::env::temp_dir().join(format!("sepika-1760-invalid-{}.ovika", std::process::id()));
    for (value, cause) in [
        (-0.008, "負値"),
        (f64::NAN, "非有限"),
        (f64::INFINITY, "非有限"),
        (f64::NEG_INFINITY, "非有限"),
    ] {
        let mut model = model.clone();
        model.quantity_cfg.beam_main_ratio = value;
        model.quantity_cfg.beam_stirrup_ratio = value;
        model.quantity_cfg.slab_rebar_ratio = value;
        sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
        let mut app = App::default();
        app.open_project_from(path.clone());
        std::fs::remove_file(&path).unwrap();
        assert!(app.core.scoped.last_error.is_none());

        let csv = crate::summary::build_quantity_csv(&app.core.model);
        let lines: Vec<_> = csv.lines().collect();
        assert_eq!(lines.len(), 3, "{csv}");
        assert_eq!(lines[..2], ["[数量積算 未算定]", "理由"]);
        let panel = quantity_panel_text(&mut app);
        assert!(panel.contains("数量積算は未算定です"), "{panel}");
        assert!(!panel.contains("コンクリート "), "{panel}");
        for name in [
            "主架構小梁の主筋比",
            "主架構小梁のスターラップ比",
            "床・片持ち床の鉄筋比",
        ] {
            let diagnostic = format!("{name}が{cause}です ({value})");
            assert!(lines[2].contains(&diagnostic), "{csv}");
            assert!(panel.contains(&diagnostic), "{panel}");
        }
        let report = crate::summary::build_report_csv(&app);
        assert!(report.contains(&csv), "{report}");
        assert!(!report.contains("[数量積算 明細]"), "{report}");

        app.ui.view.design_view = DesignView::Quantities;
        let output = frame(&mut app, vec![]);
        let button_position = |label| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => {
                        Some(text.visual_bounding_rect().center())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("操作が見つからない: {label}"))
        };
        button_position("💾 CSV エクスポート…");
        let pos = button_position("📋 クリップボードへコピー");
        let click = |pressed| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        frame(&mut app, click(true));
        let copied = frame(&mut app, click(false));
        assert!(copied
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == &csv)));

        assert!(app.core.scoped.last_error.is_none());
        let cfg = app.core.model.quantity_cfg;
        assert_eq!(
            [
                cfg.beam_main_ratio.to_bits(),
                cfg.beam_stirrup_ratio.to_bits(),
                cfg.slab_rebar_ratio.to_bits(),
            ],
            [value.to_bits(); 3],
        );
    }
}

#[test]
fn saved_zero_quantity_ratios_are_calculated_as_zero() {
    use sepika_design_jp::quantity::{
        try_compute_quantity_takeoff, MemberCategory, QuantityStatus, RebarUsage,
    };

    let mut model = quantity_ratio_fixture();
    model.quantity_cfg.beam_main_ratio = 0.0;
    model.quantity_cfg.beam_stirrup_ratio = 0.0;
    model.quantity_cfg.slab_rebar_ratio = 0.0;
    let path = std::env::temp_dir().join(format!("sepika-1760-zero-{}.ovika", std::process::id()));
    sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
    let mut app = App::default();
    app.open_project_from(path.clone());
    std::fs::remove_file(&path).unwrap();
    assert!(app.core.scoped.last_error.is_none());

    let model = &app.core.model;
    let takeoff = try_compute_quantity_takeoff(model, &model.quantity_cfg).unwrap();
    let csv = crate::summary::build_quantity_csv(model);
    assert!(csv.contains("[数量積算 明細]"), "{csv}");
    assert!(!csv.contains("[数量積算 未算定]"), "{csv}");
    for (category, usage, expected_count) in [
        (MemberCategory::Beam, RebarUsage::BeamMain, 2),
        (MemberCategory::Beam, RebarUsage::BeamStirrup, 2),
        (MemberCategory::Slab, RebarUsage::SlabBar, 1),
        (MemberCategory::CantileverSlab, RebarUsage::SlabBar, 1),
    ] {
        let items: Vec<_> = takeoff
            .items
            .iter()
            .filter(|item| item.category == category)
            .collect();
        assert_eq!(items.len(), expected_count);
        for item in items {
            assert!(item.concrete_m3 > 0.0);
            assert_eq!(item.rebar_status(), QuantityStatus::Complete);
            let rebar = item
                .rebar
                .iter()
                .find(|rebar| rebar.usage == usage)
                .unwrap();
            assert_eq!(rebar.weight_t, 0.0);
        }
    }
    for label in ["小梁主筋", "小梁スターラップ", "床筋"] {
        let row_suffix = format!(",{label},,0.000000,0.000000,算定済み概算,");
        assert_eq!(
            csv.lines()
                .filter(|line| line.ends_with(&row_suffix))
                .count(),
            2,
            "{csv}"
        );
    }
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
