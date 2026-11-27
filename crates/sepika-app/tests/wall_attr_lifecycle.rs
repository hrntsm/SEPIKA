use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    wall_is_seismic, ElementData, ElementKind, EndCondition, ForceRegime, LoadCfg, LocalAxis,
    Material, MaterialCategory, Model, Node, WallAttr, WallOpening, WallSlit,
};
use sepika_core::section_shape::SectionShape;
use sepika_core::units::GRAVITY_MM_S2;
use sepika_edit::{AddMember, DeleteMember, UndoStack};
use sepika_io::ovika::{load_ovika, save_ovika};

fn element(id: u32, kind: ElementKind, nodes: &[u32]) -> ElementData {
    ElementData {
        id: ElemId(id),
        kind,
        nodes: nodes.iter().copied().map(NodeId).collect(),
        section: None,
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed; 2],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        end_offsets_mm: [[0.0; 3]; 2],
        plastic_zone: None,
        spring: None,
    }
}

fn adjacent_walls() -> Model {
    let material = Material {
        id: MaterialId(0),
        name: "Fc24".into(),
        category: MaterialCategory::Concrete,
        young: 23_000.0,
        poisson: 0.2,
        density: 0.000_024 / GRAVITY_MM_S2,
        shear: None,
        fc: Some(24.0),
        fy: None,
        concrete_class: Default::default(),
        strength_factor: None,
    };
    let mut section = SectionShape::RcWall {
        thickness: 150.0,
        pwh_ratio: None,
        pwv_ratio: None,
        ps: 0.0025,
    }
    .to_section(SectionId(0), "W150".into());
    section.material = Some(material.id);
    let mut panels = [
        element(0, ElementKind::Wall, &[0, 1, 4, 3]),
        element(1, ElementKind::Wall, &[1, 2, 5, 4]),
    ];
    for panel in &mut panels {
        panel.section = Some(section.id);
    }
    Model {
        nodes: (0..6)
            .map(|id| Node {
                id: NodeId(id),
                coord: [f64::from(id % 3) * 1000.0, 0.0, f64::from(id / 3) * 3000.0],
                restraint: Dof6Mask::FREE,
                story: None,
                mass: None,
                support_spring: None,
            })
            .collect(),
        elements: panels.into(),
        materials: vec![material],
        sections: vec![section],
        wall_attrs: vec![
            WallAttr {
                elem: ElemId(0),
                opening_area: 100_000.0,
                opening_weight: 20.0,
                openings: vec![],
                slit: Default::default(),
                finish_intensity: 0.0005,
            },
            WallAttr {
                elem: ElemId(1),
                opening_area: 300_000.0,
                opening_weight: 80.0,
                openings: vec![],
                slit: Default::default(),
                finish_intensity: 0.001,
            },
            WallAttr {
                elem: ElemId(0),
                opening_area: 200_000.0,
                opening_weight: 100.0,
                openings: vec![WallOpening {
                    width: 800.0,
                    height: 1000.0,
                    offset: Some([100.0, 300.0]),
                }],
                slit: WallSlit {
                    column_face: [true, false],
                    beam_face: [false, true],
                },
                finish_intensity: 0.002,
            },
            WallAttr {
                elem: ElemId(1),
                opening_area: 400_000.0,
                opening_weight: 125.0,
                openings: vec![WallOpening {
                    width: 500.0,
                    height: 500.0,
                    offset: Some([250.0, 1400.0]),
                }],
                slit: WallSlit {
                    column_face: [false, true],
                    beam_face: [true, false],
                },
                finish_intensity: 0.003,
            },
        ],
        ..Default::default()
    }
}

fn ovika_roundtrip(model: &Model, label: &str) -> Model {
    let path = std::env::temp_dir().join(format!(
        "sepika-wall-attrs-{label}-{}.ovika",
        std::process::id()
    ));
    save_ovika(&path, model, Default::default()).unwrap();
    let restored = load_ovika(&path).unwrap().model;
    std::fs::remove_file(path).unwrap();
    assert_eq!(restored.wall_attrs, model.wall_attrs);
    assert_eq!(
        restored
            .load_cfg
            .as_ref()
            .map(|cfg| &cfg.finish_area_weight),
        model.load_cfg.as_ref().map(|cfg| &cfg.finish_area_weight)
    );
    restored
}

#[test]
fn deleting_a_wall_preserves_the_adjacent_wall_and_undo_restores_all_ordered_input() {
    let mut model = adjacent_walls();
    model.elements.extend([
        element(2, ElementKind::Beam, &[1, 2]),
        element(3, ElementKind::Beam, &[4, 5]),
    ]);
    model.load_cfg = Some(LoadCfg {
        finish_area_weight: vec![
            (ElemId(0), 0.001),
            (ElemId(1), 0.002),
            (ElemId(0), 0.003),
            (ElemId(2), 0.004),
        ],
        ..Default::default()
    });
    let initial = model.clone();
    let mut model = ovika_roundtrip(&model, "walls");
    assert!(wall_is_seismic(&model.elements[1], &model));
    let remaining_attrs = [1, 3].map(|i| WallAttr {
        elem: ElemId(0),
        ..initial.wall_attrs[i].clone()
    });
    let mut undo = UndoStack::new();
    assert!(undo.run(&mut model, Box::new(DeleteMember { id: ElemId(0) })));

    let assert_remaining = |model: &Model| {
        let wall = model.element(ElemId(0)).unwrap();
        assert_eq!(wall.nodes, initial.elements[1].nodes);
        assert_eq!(model.wall_attrs, remaining_attrs);
        assert_eq!(model.wall_attrs[0].total_opening_area(), 300_000.0);
        assert!(wall_is_seismic(wall, model));
        assert_eq!(
            model.load_cfg.as_ref().unwrap().finish_area_weight,
            [(ElemId(0), 0.002), (ElemId(1), 0.004)]
        );
    };
    assert_remaining(&ovika_roundtrip(&model, "walls"));

    undo.undo(&mut model);
    assert_eq!(undo.last_error(), None);
    let restored = ovika_roundtrip(&model, "walls");
    assert!(restored.eq_ignoring_dofmap(&initial));
    assert!(wall_is_seismic(&restored.elements[1], &restored));

    undo.redo(&mut model);
    assert_eq!(undo.last_error(), None);
    assert_remaining(&ovika_roundtrip(&model, "walls"));
}

#[test]
fn invalid_wall_attributes_stop_member_editing_preparation_and_analysis() {
    let mut baseline = adjacent_walls();
    baseline
        .elements
        .push(element(2, ElementKind::Beam, &[1, 2]));
    let added = element(3, ElementKind::Wall, &[0, 1, 4, 3]);
    for id in [ElemId(2), ElemId(3)] {
        let mut model = baseline.clone();
        model.wall_attrs[3].elem = id;
        let before = model.clone();
        let mut undo = UndoStack::new();
        for command in [
            Box::new(AddMember {
                elem: added.clone(),
            }) as Box<dyn sepika_edit::EditCommand>,
            Box::new(DeleteMember { id: ElemId(0) }),
        ] {
            assert!(!undo.run(&mut model, command));
            let error = undo.last_error().unwrap();
            assert!(error.contains("壁属性") && error.contains(&format!("部材 {}", id.0)));
        }
        assert!(!undo.can_undo());

        let errors = [
            sepika_job::prepare::prepare_model_for_analysis(&mut model, &Default::default(), None)
                .err()
                .unwrap()
                .to_string(),
            sepika_job::compute::compute_linear_static(model.clone(), LoadCaseId(0))
                .err()
                .unwrap()
                .to_string(),
            sepika_solver::statics::analysis::Analysis::prepare(&model)
                .err()
                .unwrap()
                .to_string(),
            sepika_load::self_weight::self_weight_case_content(&model, &LoadCfg::default())
                .unwrap_err(),
        ];
        for error in errors {
            assert!(error.contains("壁属性"), "{error}");
            assert!(error.contains(&format!("部材 {}", id.0)), "{error}");
        }
        assert!(model.eq_ignoring_dofmap(&before));
    }
}
