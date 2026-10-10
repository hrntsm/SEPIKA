fn circular_post_model() -> sepika_core::model::Model {
    use sepika_core::ids::{MaterialId, NodeId, SecondaryMemberId, SectionId};
    use sepika_core::model::{
        Material, MaterialCategory, Model, Node, SecondaryMember, SecondaryMemberEnds,
        SecondaryMemberKind,
    };
    use sepika_core::section_shape::{CircleColumnHoop, RcCircleColumnRebar, SectionShape};
    let mut section = SectionShape::RcColumnCircle {
        d: 400.0,
        rebar: RcCircleColumnRebar {
            main_dia: 25.0,
            count: 0,
            cover: 40.0,
            hoop: CircleColumnHoop {
                dia: 10.0,
                pitch: 100.0,
            },
        },
    }
    .to_section(SectionId(0), "円形断面".into());
    section.material = Some(MaterialId(0));
    section.frame_use = Some(sepika_core::model::FrameSectionUse::Column);
    section.width = 900.0;
    section.depth = 700.0;
    let ends = [[0.0, 0.0, 0.0], [0.0, 0.0, 3000.0]];
    Model {
        nodes: ends
            .into_iter()
            .enumerate()
            .map(|(id, coord)| Node {
                id: NodeId(id as u32),
                coord,
                restraint: sepika_core::dof::Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        sections: vec![section],
        materials: vec![Material {
            id: MaterialId(0),
            name: "Fc24".into(),
            category: MaterialCategory::Concrete,
            young: 22700.0,
            poisson: 0.2,
            density: 2.4e-9,
            shear: None,
            fc: Some(24.0),
            fy: None,
            concrete_class: Default::default(),
            strength_factor: None,
        }],
        unassigned_posts: vec![SecondaryMember {
            id: SecondaryMemberId(448),
            kind: SecondaryMemberKind::Post,
            ends: SecondaryMemberEnds::Detached(ends),
            section: Some(SectionId(0)),
            name: "間柱符号".into(),
            gravity_end_shares: None,
        }],
        ..Default::default()
    }
}

#[test]
fn circular_post_quantity_csv_uses_shared_takeoff_and_reports_invalid_geometry() {
    use sepika_core::section_shape::SectionShape;
    let mut model = circular_post_model();
    let q = sepika_design_jp::quantity::try_compute_quantity_takeoff(
        &model,
        &sepika_design_jp::quantity::QuantityCfg::default(),
    )
    .unwrap();
    assert!((q.totals().concrete_m3 - 0.3769911184).abs() < 1e-9);
    assert!((q.totals().formwork_m2 - 3.7699111843).abs() < 1e-9);
    let csv = sepika_app::summary::build_quantity_csv(&model);
    assert!(csv.contains("柱,0.38,3.77,"), "{csv}");
    assert!(csv.contains("間柱符号,0.377,3.77,"), "{csv}");
    if let Some(SectionShape::RcColumnCircle { d, .. }) = &mut model.sections[0].shape {
        *d = f64::NAN;
    }
    let csv = sepika_app::summary::build_quantity_csv(&model);
    assert!(csv.starts_with("[数量積算 未算定]"), "{csv}");
    assert!(csv.contains("SecondaryMemberId(448)"), "{csv}");
    assert!(csv.contains("直径 D"), "{csv}");
}

#[cfg(feature = "gui")]
#[test]
fn circular_post_quantity_gui_panel_uses_shared_takeoff() {
    fn panel_text(model: sepika_core::model::Model) -> String {
        let mut app = sepika_app::app::App::default();
        app.core.model = model;
        let context = egui::Context::default();
        let output = context.run_ui(egui::RawInput::default(), |root_ui| {
            egui::CentralPanel::default().show_inside(root_ui, |ui| {
                sepika_app::quantity_view::quantity_panel(ui, &mut app);
            });
        });
        output
            .shapes
            .into_iter()
            .filter_map(|shape| match shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    let mut model = circular_post_model();
    let text = panel_text(model.clone());
    assert!(text.contains("0.38"), "{text}");
    assert!(text.contains("3.77"), "{text}");
    model.unassigned_posts[0].ends =
        sepika_core::model::SecondaryMemberEnds::Detached([[0.0; 3]; 2]);
    let text = panel_text(model);
    assert!(text.contains("数量積算は未算定"), "{text}");
    assert!(text.contains("SecondaryMemberId(448)"), "{text}");
    assert!(text.contains("実長 L"), "{text}");
}
