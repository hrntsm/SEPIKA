fn beam_contact_fixture() -> sepika_core::model::Model {
    use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId, SlabId};
    use sepika_core::model::*;
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};
    let mut beam = SectionShape::RcBeamRect {
        b: 300.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: 0.0,
            top: vec![],
            bottom: vec![],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 0.0,
                pitch: 0.0,
                legs: 0,
            },
        },
    }
    .to_section(SectionId(0), "G451".into());
    beam.material = Some(MaterialId(0));
    beam.frame_use = Some(FrameSectionUse::Girder);
    let mut sections = vec![beam];
    for (id, t) in [(1, 150.0), (2, 100.0)] {
        let mut section =
            SectionShape::RcSlab { thickness: t }.to_section(SectionId(id), "床".into());
        section.material = Some(MaterialId(0));
        sections.push(section);
    }
    Model {
        nodes: [[0.0, 0.0, 3000.0], [6000.0, 0.0, 3000.0]]
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
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            plastic_zone: None,
            spring: None,
        }],
        sections,
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
        slabs: [(0, 1, 2000.0), (1, 2, -2000.0)]
            .into_iter()
            .map(|(id, sec, extent)| Slab {
                id: SlabId(id),
                shape: SlabShape::Attached {
                    anchor: RegionAnchor::Line {
                        nodes: [NodeId(0), NodeId(1)],
                        span: [0.0, 1.0],
                        transfer: LoadTransfer::Anchor,
                    },
                    extent: [extent; 2],
                },
                plate: SlabPlate {
                    section: Some(SectionId(sec)),
                    ..Default::default()
                },
                tip_loads: vec![],
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn beam_contact_quantity_csv_preserves_union_and_diagnostics() {
    for model in beam_contact_narrow_fixtures() {
        let csv = sepika_app::summary::build_quantity_csv(&model);
        assert!(csv.contains("G451,1.080,8.10,"), "{csv}");
    }
    let no_plate = sepika_app::summary::build_quantity_csv(&beam_contact_no_plate_fixture());
    assert!(no_plate.contains("G451,1.080,8.40,"), "{no_plate}");
    let model = beam_contact_fixture();
    let csv = sepika_app::summary::build_quantity_csv(&model);
    assert!(csv.contains("G451,1.080,7.50,"), "{csv}");
    for (invalid, expected_reason) in beam_contact_invalid_cases() {
        let csv = sepika_app::summary::build_quantity_csv(&invalid);
        assert!(
            csv.contains("数量積算 未算定") && csv.contains(expected_reason),
            "{csv}"
        );
        assert!(!csv.contains("G451,"), "{csv}");
    }
}

#[cfg(feature = "gui")]
#[test]
fn beam_contact_quantity_gui_renders_union_and_unavailable_reason() {
    fn panel_text(model: sepika_core::model::Model) -> String {
        let mut app = sepika_app::app::App::default();
        app.core.model = model;
        app.ui.view.quantity_view.grouping = sepika_app::quantity_view::QuantityGrouping::Detail;
        let context = egui::Context::default();
        context
            .run_ui(egui::RawInput::default(), |root| {
                egui::CentralPanel::default().show_inside(root, |ui| {
                    sepika_app::quantity_view::quantity_panel(ui, &mut app)
                });
            })
            .shapes
            .into_iter()
            .filter_map(|shape| match shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    for model in beam_contact_narrow_fixtures() {
        let text = panel_text(model);
        assert!(text.contains("G451") && text.contains("8.10"), "{text}");
    }
    let no_plate = panel_text(beam_contact_no_plate_fixture());
    assert!(
        no_plate.contains("G451") && no_plate.contains("8.40"),
        "{no_plate}"
    );
    let model = beam_contact_fixture();
    let text = panel_text(model.clone());
    assert!(text.contains("G451") && text.contains("7.50"), "{text}");
    for (invalid, expected_reason) in beam_contact_invalid_cases() {
        let text = panel_text(invalid);
        assert!(
            text.contains("数量積算は未算定") && text.contains(expected_reason),
            "{text}"
        );
        assert!(!text.contains("7.50"), "{text}");
    }
}

fn beam_contact_invalid_cases() -> Vec<(sepika_core::model::Model, &'static str)> {
    use sepika_core::ids::{FloorPlateAssignmentRegionId, NodeId, SectionId, SlabId};
    use sepika_core::model::{
        FloorPlateAssignmentRegion, PlateAssignment, RegionAnchor, SlabShape,
    };
    let base = beam_contact_fixture();
    let mut cases = Vec::new();
    let mut model = base.clone();
    model.elements[0].section = Some(SectionId(999));
    cases.push((model, "Primary(ElemId(0)): 梁断面 SectionId(999)"));
    let mut model = base.clone();
    for i in 0..2 {
        let mut node = model.nodes[i].clone();
        node.id = NodeId(i as u32 + 2);
        node.coord[2] = 3100.0;
        model.nodes.push(node);
    }
    for slab in &mut model.slabs {
        if let SlabShape::Attached {
            anchor: RegionAnchor::Line { nodes, .. },
            ..
        } = &mut slab.shape
        {
            *nodes = [NodeId(2), NodeId(3)];
        }
    }
    model.nodes[1].coord[2] += 0.5;
    cases.push((model.clone(), "高さが一定でない梁"));
    model.elements[0].nodes.reverse();
    cases.push((model, "高さが一定でない梁"));

    for t in [-1.0, 0.0, f64::NAN, f64::INFINITY] {
        let mut model = base.clone();
        model.sections[1].thickness = Some(t);
        cases.push((model, "実厚"));
    }
    for assignment in [PlateAssignment::Unset, PlateAssignment::Plate(SlabId(999))] {
        let mut model = base.clone();
        model
            .floor_assignment_regions
            .regions
            .push(FloorPlateAssignmentRegion {
                id: FloorPlateAssignmentRegionId(451),
                boundary: vec![],
                assignment,
            });
        cases.push((
            model,
            if assignment.is_unset() {
                "Unset"
            } else {
                "床板"
            },
        ));
    }
    let mut model = base.clone();
    model.slabs[0].plate.section = Some(SectionId(999));
    cases.push((model, "断面"));
    let mut model = base.clone();
    if let SlabShape::Attached {
        anchor: RegionAnchor::Line { nodes, .. },
        ..
    } = &mut model.slabs[0].shape
    {
        nodes[0] = NodeId(999);
    }
    cases.push((model, "参照"));
    let mut model = base.clone();
    model.sections[0].shape = None;
    cases.push((model, "矩形"));
    let mut model = base.clone();
    if let SlabShape::Attached { extent, .. } = &mut model.slabs[0].shape {
        *extent = [2000.0, -2000.0];
    }
    cases.push((model, "自己交差"));
    cases
}

fn beam_contact_narrow_fixtures() -> Vec<sepika_core::model::Model> {
    let mut models = Vec::new();
    for side in [-1.0, 1.0] {
        let mut model = beam_contact_fixture();
        model.slabs.truncate(1);
        if let sepika_core::model::SlabShape::Attached { extent, .. } = &mut model.slabs[0].shape {
            *extent = [side * 5.0; 2];
        }
        models.push(model.clone());
        model.elements[0].nodes.reverse();
        models.push(model);
    }
    models
}

fn beam_contact_no_plate_fixture() -> sepika_core::model::Model {
    let mut model = beam_contact_fixture();
    model.slabs.remove(0);
    model
        .floor_assignment_regions
        .regions
        .push(sepika_core::model::FloorPlateAssignmentRegion {
            id: sepika_core::ids::FloorPlateAssignmentRegionId(451),
            boundary: vec![],
            assignment: sepika_core::model::PlateAssignment::NoPlate,
        });
    model
}
