use sepika_app::sample::portal_frame;
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{DamperProps, DamperSpec, ElementKind, LoadCfg, Model, DL_CASE_NAME};
use sepika_core::section_shape::SectionShape;
use sepika_edit::{AddDamper, AddMember, DeleteMember, EditCommand, UndoStack};
use sepika_io::ovika::{load_ovika, save_ovika};
use sepika_job::{apply_rigid_zones_and_panels, prepare_model_for_analysis};

fn columns_with_member_weights() -> Model {
    let mut model = portal_frame();
    let shape = SectionShape::SteelH {
        height: 500.0,
        width: 300.0,
        web_thick: 10.0,
        flange_thick: 15.0,
        root_r: Some(0.0),
    };
    let mut section = shape.to_section(model.sections[0].id, "H-500x300x10x15".into());
    section.frame_use = model.sections[0].frame_use;
    section.material = model.sections[0].material;
    model.sections = vec![section];
    model.stories[1].elevation = Some(3000.0);
    model.load_cases.clear();
    let bottom = model.nodes[0].clone();
    let top = model.nodes[2].clone();
    let column = model.elements[0].clone();
    model.nodes.clear();
    for index in 0..5 {
        let x_mm = f64::from(index) * 4000.0;
        let mut bottom = bottom.clone();
        bottom.id = NodeId(2 * index);
        bottom.coord = [x_mm, 0.0, 0.0];
        let mut top = top.clone();
        top.id = NodeId(2 * index + 1);
        top.coord = [x_mm, 0.0, 3000.0];
        model.nodes.extend([bottom, top]);
    }
    model.elements = [2, 0, 4, 6]
        .into_iter()
        .enumerate()
        .map(|(index, bottom)| {
            let mut element = column.clone();
            element.id = ElemId(index as u32);
            element.nodes = [NodeId(bottom), NodeId(bottom + 1)].into_iter().collect();
            element
        })
        .collect();
    model.load_cfg = Some(LoadCfg {
        extra_line_weight: vec![(ElemId(0), 0.7), (ElemId(2), 0.4)],
        finish_area_weight: vec![(ElemId(0), 0.003), (ElemId(3), 0.002)],
        dampers: vec![
            DamperSpec {
                elem: ElemId(0),
                total_weight: 5000.0,
            },
            DamperSpec {
                elem: ElemId(1),
                total_weight: 4000.0,
            },
        ],
        ..Default::default()
    });
    model
}

fn assert_close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}

#[test]
fn deleting_and_reusing_a_number_preserves_dl_and_seismic_weight_after_ovika_roundtrip() {
    let mut model = columns_with_member_weights();
    let original_elements = model.elements.clone();
    let original_weights = model.load_cfg.clone();
    let mut edits = UndoStack::new();
    assert!(edits.run(&mut model, Box::new(DeleteMember { id: ElemId(0) })));

    let deleted_weights = LoadCfg {
        extra_line_weight: vec![(ElemId(1), 0.4)],
        finish_area_weight: vec![(ElemId(2), 0.002)],
        dampers: vec![DamperSpec {
            elem: ElemId(0),
            total_weight: 4000.0,
        }],
        ..Default::default()
    };
    assert_eq!(model.load_cfg.as_ref(), Some(&deleted_weights));
    assert_eq!(model.elements[0].nodes, original_elements[1].nodes);
    assert_eq!(model.elements[1].nodes, original_elements[2].nodes);
    assert_eq!(model.elements[2].nodes, original_elements[3].nodes);

    let mut new_column = original_elements[0].clone();
    new_column.id = ElemId(3);
    new_column.nodes = [NodeId(8), NodeId(9)].into_iter().collect();
    assert!(edits.run(&mut model, Box::new(AddMember { elem: new_column })));
    assert_eq!(model.load_cfg.as_ref(), Some(&deleted_weights));
    let edited_elements = model.elements.clone();

    edits.undo(&mut model);
    edits.undo(&mut model);
    assert_eq!(model.elements, original_elements);
    assert_eq!(model.load_cfg, original_weights);
    edits.redo(&mut model);
    assert_eq!(model.load_cfg.as_ref(), Some(&deleted_weights));
    edits.redo(&mut model);
    assert_eq!(model.elements, edited_elements);
    assert_eq!(model.load_cfg.as_ref(), Some(&deleted_weights));

    let path = std::env::temp_dir().join(format!(
        "sepika-member-weight-references-{}.ovika",
        std::process::id()
    ));
    save_ovika(&path, &model, Default::default()).unwrap();
    let mut loaded = load_ovika(&path).unwrap().model;
    std::fs::remove_file(path).unwrap();
    assert_eq!(loaded.elements, edited_elements);
    assert_eq!(loaded.load_cfg.as_ref(), Some(&deleted_weights));
    prepare_model_for_analysis(&mut loaded, &Default::default(), None).unwrap();
    let dead = loaded
        .load_cases
        .iter()
        .find(|case| case.name == DL_CASE_NAME)
        .unwrap();
    assert!(dead.member.is_empty());
    // 柱自重は片端 1613.175 N、追加分は 600 N と 4800 N。ダンパー指定柱は片端 2000 N。
    let nodal_weights = [2000.0, 0.0, 2213.175, 6413.175, 1613.175];
    for (index, expected) in nodal_weights.into_iter().enumerate() {
        for end in [0, 1] {
            let node = NodeId((2 * index + end) as u32);
            let weight_n: f64 = dead
                .nodal
                .iter()
                .filter(|load| load.node == node)
                .map(|load| -load.values[2])
                .sum();
            assert_close(weight_n, expected);
        }
    }
    let seismic = loaded.seismic_weight_generation.as_ref().unwrap();
    for &(_, weight_n) in &seismic.calculated_weights {
        assert_close(weight_n, 12239.525);
    }
    assert_eq!(seismic.calculated_weights.len(), 2);
    let roof = loaded.stories[1].dynamic_mass.unwrap();
    assert_close(roof.mass_equiv_weight_n, 12145.952784125);
    assert_close(roof.center_xy_mm[0], 9826.436470714);
    assert_close(roof.center_xy_mm[1], 0.0);
}

#[test]
fn missing_weight_references_cannot_become_members_or_generated_panels() {
    let model = portal_frame();
    let mut preview = model.clone();
    let panels = apply_rigid_zones_and_panels(&mut preview).unwrap();
    assert!(!panels.is_empty());
    let target = ElemId(model.elements.len() as u32);
    assert_eq!(
        preview.element(target).unwrap().kind,
        ElementKind::PanelZone
    );
    for (kind, cfg) in [
        (
            "部材付加線重量",
            LoadCfg {
                extra_line_weight: vec![(target, 0.4)],
                ..Default::default()
            },
        ),
        (
            "仕上げ面重量",
            LoadCfg {
                finish_area_weight: vec![(target, 0.002)],
                ..Default::default()
            },
        ),
        (
            "ダンパー総重量",
            LoadCfg {
                dampers: vec![DamperSpec {
                    elem: target,
                    total_weight: 4000.0,
                }],
                ..Default::default()
            },
        ),
    ] {
        let check_error = |error: &str| {
            assert!(error.contains(kind), "{error}");
            assert!(error.contains("部材 3"), "{error}");
        };
        let error = sepika_load::self_weight::self_weight_case_content(&model, &cfg).unwrap_err();
        check_error(&error);

        let mut invalid = model.clone();
        invalid.load_cfg = Some(cfg);
        let before = invalid.clone();
        let error = sepika_load::story_gen::generate_stories_multi(&invalid, &[]).unwrap_err();
        check_error(&error);
        let prep = prepare_model_for_analysis(&mut invalid, &Default::default(), None);
        check_error(&prep.err().unwrap().to_string());
        assert!(invalid.eq_ignoring_dofmap(&before));
        check_error(&apply_rigid_zones_and_panels(&mut invalid).unwrap_err());
        assert!(invalid.eq_ignoring_dofmap(&before));

        let mut elem = model.elements[0].clone();
        elem.id = target;
        let member = AddMember { elem: elem.clone() };
        elem.kind = ElementKind::Damper;
        elem.section = None;
        let damper = AddDamper {
            elem,
            props: DamperProps::default(),
        };
        let mut edits = UndoStack::new();
        for command in [Box::new(member) as Box<dyn EditCommand>, Box::new(damper)] {
            assert!(!edits.run(&mut invalid, command));
            check_error(edits.last_error().unwrap());
            assert_eq!(edits.undo_label(), None);
            assert!(invalid.eq_ignoring_dofmap(&before));
        }
    }
}
