use sepika_core::{dof::Dof6Mask, ids::*, model::*, section_shape::SectionShape};
use sepika_solver::statics::analysis::Analysis;
fn baseline() -> Model {
    let mut section = SectionShape::SteelFlatBar {
        width: 200.,
        thick: 20.,
    }
    .to_section(SectionId(0), "test".into());
    section.material = Some(MaterialId(0));
    section.frame_use = Some(FrameSectionUse::Girder);
    let mut m = Model {
        nodes: (0..3)
            .map(|i| Node {
                id: NodeId(i),
                coord: [3000. * i as f64, 0., 0.],
                restraint: if i != 1 {
                    Dof6Mask::FIXED
                } else {
                    Dof6Mask::FREE
                },
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0., 0., 1.],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![section],
        materials: vec![Material {
            id: MaterialId(0),
            name: "SN400B".into(),
            category: MaterialCategory::Steel,
            young: 205000.,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy: Some(235.),
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        stories: Vec::new(),
        load_cases: default_load_cases(),
        ..Default::default()
    };
    m.load_cases[0].nodal = vec![NodalLoad::manual(NodeId(1), [1000., 0., 0., 0., 0., 0.])];
    m
}

#[test]
fn missing_manual_and_auto_load_nodes_are_rejected_before_analysis() {
    for automatic in [false, true] {
        for id in [1, 3, u32::MAX] {
            let mut model = baseline();
            let load = if automatic {
                NodalLoad::auto(NodeId(id), [1000., 0., 0., 0., 0., 0.])
            } else {
                NodalLoad::manual(NodeId(id), [1000., 0., 0., 0., 0., 0.])
            };
            model.load_cases[0].nodal = vec![load];
            let before = serde_json::to_value(&model).unwrap();
            let prepared = Analysis::prepare(&model);
            if id == 1 {
                let result = prepared.unwrap().linear_static(LoadCaseId(0)).unwrap();
                assert!((result.disp[1][0] - 0.003658536585).abs() < 1e-12);
            } else {
                let error = prepared
                    .err()
                    .expect("missing node must fail prepare")
                    .to_string();
                assert!(error.contains(&id.to_string()), "{error}");
            }
            assert_eq!(serde_json::to_value(&model).unwrap(), before);
        }
    }
}
