use super::*;
use sepika_core::ids::*;
use sepika_core::model::*;
use sepika_core::section_shape::SectionShape;

fn shapes() -> [SectionShape; 2] {
    [
        SectionShape::CftBox {
            height: 400.0,
            width: 400.0,
            thick: 10.0,
            corner_r: Some(0.0),
        },
        SectionShape::CftPipe {
            outer_dia: 400.0,
            thick: 10.0,
        },
    ]
}

fn secondary_model(shape: SectionShape, kind: SecondaryMemberKind, assigned: bool) -> Model {
    let mut model = Model::with_default_load_cases();
    let mut section = shape.to_section(SectionId(0), "試験断面".into());
    section.frame_use = Some(FrameSectionUse::Column);
    model.sections.push(section);
    let sm = SecondaryMember {
        id: SecondaryMemberId(539),
        kind,
        section: Some(SectionId(0)),
        ends: SecondaryMemberEnds::Detached([[0.0, 0.0, 0.0], [0.0, 0.0, 3000.0]]),
        ..SecondaryMember::default()
    };
    match (kind, assigned) {
        (SecondaryMemberKind::Beam, false) => model.unassigned_beams.push(sm),
        (SecondaryMemberKind::Post, false) => model.unassigned_posts.push(sm),
        (SecondaryMemberKind::Beam, true) => {
            let mut r = FloorRegion::new(FloorRegionId(0), vec![]);
            r.secondary_beams.push(sm);
            model.floor_regions.push(r);
        }
        (SecondaryMemberKind::Post, true) => {
            let mut r = WallRegion::new(WallRegionId(0), vec![]);
            r.posts.push(sm);
            model.wall_regions.push(r);
        }
    }
    model
}

// 正規保存入口を迂回し、ハッシュ整合のある不正モデルを読込入口へ渡す必要がある。
pub(super) fn write_invalid_container(path: &std::path::Path, model: &Model) {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let model_bytes = rmp_serde::to_vec_named(model).unwrap();
    let settings = b"{}";
    let manifest = serde_json::json!({
        "schema_version": sepika_io::ovika::CURRENT_SCHEMA_VERSION,
        "units": "internal: N-mm-s", "created_by": "Issue539 test",
        "entries": [
            {"name": "model.msgpack", "sha256": format!("{:x}", Sha256::digest(&model_bytes))},
            {"name": "settings.json", "sha256": format!("{:x}", Sha256::digest(settings))}
        ]
    });
    let mut archive = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let options = zip::write::FileOptions::<()>::default();
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("model.msgpack", model_bytes),
        ("settings.json", settings.to_vec()),
    ] {
        archive.start_file(name, options).unwrap();
        archive.write_all(&bytes).unwrap();
    }
    archive.finish().unwrap();
}

#[test]
fn secondary_cft_public_entries_reject_all_kinds_shapes_and_locations() {
    use sepika_design_jp::quantity::{try_compute_quantity_takeoff, QuantityCfg};
    for (shape_idx, shape) in shapes().into_iter().enumerate() {
        for kind in [SecondaryMemberKind::Beam, SecondaryMemberKind::Post] {
            for assigned in [false, true] {
                let model = secondary_model(shape.clone(), kind, assigned);
                let mut control = model.clone();
                let mut steel_section = SectionShape::SteelBox {
                    height: 400.0,
                    width: 400.0,
                    thick: 10.0,
                    corner_r: Some(0.0),
                }
                .to_section(SectionId(0), "正常対照".into());
                steel_section.frame_use = Some(FrameSectionUse::Column);
                control.sections[0] = steel_section;
                control.validate().unwrap();
                let reason =
                    try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
                assert!(reason.contains("SecondaryMemberId(539)"));
                assert!(reason.contains(&format!("{kind:?}")));
                assert!(reason.contains("SectionId(0)"));
                let csv = crate::summary::build_quantity_csv(&model);
                assert!(csv.contains("[数量積算 未算定]"));
                assert!(csv.contains(&reason));
                assert!(!csv.contains("[数量積算 部位別]"));
                let error =
                    sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(0))
                        .unwrap_err()
                        .to_string();
                assert!(error.contains("SecondaryMemberId(539)"), "{error}");
                let error = Analysis::prepare(&model).err().unwrap().to_string();
                assert!(error.contains("SecondaryMemberId(539)"), "{error}");
                let dofmap = sepika_core::dof::DofMap::build(&model);
                let reducer = sepika_solver::common::constraint::Reducer::empty();
                let error =
                    sepika_solver::dynamic::eigen::solve_eigen(&model, &dofmap, &reducer, 0)
                        .unwrap_err()
                        .to_string();
                assert!(error.contains("SecondaryMemberId(539)"), "{error}");
                let error = sepika_solver::nonlinear::pushover::pushover_analysis(
                    &model,
                    &dofmap,
                    &reducer,
                    SeismicDir::X,
                    1,
                    1.0,
                    false,
                )
                .unwrap_err();
                assert!(error.contains("SecondaryMemberId(539)"), "{error}");
                let wave = sepika_solver::dynamic::timehistory::GroundMotion {
                    dt: 0.01,
                    accel_x: vec![0.0],
                    accel_y: None,
                    accel_theta: None,
                };
                let cfg = sepika_solver::dynamic::timehistory::NewmarkCfg::average_accel();
                let damping = sepika_solver::dynamic::damping::Damping::MassProportional {
                    h: 0.02,
                    omega: 1.0,
                };
                let error = sepika_solver::dynamic::timehistory::linear_time_history_analysis(
                    &model,
                    &dofmap,
                    &reducer,
                    &wave,
                    &cfg,
                    &damping,
                    &[],
                    &[],
                    false,
                    None,
                )
                .unwrap_err()
                .to_string();
                assert!(error.contains("SecondaryMemberId(539)"), "{error}");
                let error = sepika_solver::dynamic::timehistory::nonlinear_time_history_analysis(
                    &model,
                    &dofmap,
                    &reducer,
                    &wave,
                    &cfg,
                    &damping,
                    sepika_solver::dynamic::damping::DampingAccumulation::NonCumulative,
                    &[],
                    &[],
                    Default::default(),
                )
                .unwrap_err()
                .to_string();
                assert!(error.contains("SecondaryMemberId(539)"), "{error}");
                let pushover: sepika_solver::nonlinear::pushover::PushoverResult = serde_json::from_value(serde_json::json!({
                    "wall_capacities": [], "steps": [], "capacity_curve": [], "hinges": [], "shear_yields": [], "mechanism": "Overall", "qu": 0.0, "member_response": []
                })).unwrap();
                let error = sepika_solver::dynamic::lumped_mass::build_lumped_mass_model(
                    &model,
                    &pushover,
                    sepika_solver::dynamic::lumped_mass::LumpedMassType::default(),
                    0.5,
                )
                .unwrap_err()
                .to_string();
                assert!(error.contains("SecondaryMemberId(539)"), "{error}");
                let error = sepika_solver::statics::analysis::seismic_distribution_for_model(
                    &model,
                    Default::default(),
                    1.0,
                )
                .unwrap_err()
                .to_string();
                assert!(error.contains("SecondaryMemberId(539)"), "{error}");
                let path = std::env::temp_dir().join(format!(
                    "sepika539-{}-{shape_idx}-{kind:?}-{assigned}.ovika",
                    std::process::id()
                ));
                let _ = std::fs::remove_file(&path);
                let error = sepika_io::ovika::save_ovika(&path, &model, Default::default())
                    .unwrap_err()
                    .to_string();
                assert!(error.contains("SecondaryMemberId(539)"), "{error}");
                assert!(!path.exists());
                assert!(!path.with_extension("ovika.tmp").exists());
                std::fs::write(&path, b"existing project bytes").unwrap();
                assert!(sepika_io::ovika::save_ovika(&path, &model, Default::default()).is_err());
                assert_eq!(std::fs::read(&path).unwrap(), b"existing project bytes");
                write_invalid_container(&path, &model);
                let error = sepika_io::ovika::load_ovika(&path)
                    .err()
                    .unwrap()
                    .to_string();
                assert!(error.contains("SecondaryMemberId(539)"), "{error}");
                let mut app = App::default();
                assert!(app.load_model(crate::sample::portal_frame()));
                app.run_linear_static(LoadCaseId(0));
                assert!(
                    app.core.scoped.results.is_some(),
                    "{:?}",
                    app.core.scoped.last_error
                );
                app.ui.scoped.selection = GeometrySelection::Nodes {
                    ids: vec![NodeId(0)],
                    active: Some(NodeId(0)),
                };
                assert!(app.core.scoped.undo.run(
                    &mut app.core.model,
                    Box::new(sepika_edit::SetNodeCoord {
                        node: NodeId(0),
                        coord: [-100.0, 0.0, 0.0]
                    })
                ));
                let before = rmp_serde::to_vec_named(&app.core.model).unwrap();
                let results = rmp_serde::to_vec_named(&app.core.scoped.results).unwrap();
                let preparation = rmp_serde::to_vec_named(&app.core.scoped.preparation).unwrap();
                let revision = app.core.scoped.undo.revision();
                for raw_open in [false, true] {
                    if raw_open {
                        app.open_project_from(path.clone());
                    } else {
                        assert!(!app.load_model(model.clone()));
                    }
                    assert!(app
                        .core
                        .scoped
                        .last_error
                        .as_ref()
                        .unwrap()
                        .contains("SecondaryMemberId(539)"));
                    assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), before);
                    assert_eq!(
                        rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
                        results
                    );
                    assert_eq!(
                        rmp_serde::to_vec_named(&app.core.scoped.preparation).unwrap(),
                        preparation
                    );
                    assert_eq!(
                        app.ui.scoped.selection,
                        GeometrySelection::Nodes {
                            ids: vec![NodeId(0)],
                            active: Some(NodeId(0))
                        }
                    );
                    assert_eq!(app.core.scoped.undo.revision(), revision);
                    assert!(app.core.scoped.undo.can_undo());
                }
                app.core.scoped.undo.undo(&mut app.core.model);
                assert_eq!(app.core.model.nodes[0].coord, [0.0, 0.0, 0.0]);
                std::fs::remove_file(path).unwrap();
            }
        }
    }
}

#[cfg(feature = "gui")]
#[test]
fn secondary_cft_gui_quantity_shows_reason_without_totals() {
    for shape in shapes() {
        for kind in [SecondaryMemberKind::Beam, SecondaryMemberKind::Post] {
            let mut app = App::default();
            app.core.model = secondary_model(shape.clone(), kind, false);
            let output = egui::Context::default().run_ui(Default::default(), |ui| {
                crate::quantity_view::quantity_panel(ui, &mut app);
            });
            fn texts(shape: &egui::epaint::Shape, out: &mut Vec<String>) {
                match shape {
                    egui::epaint::Shape::Text(text) => out.push(text.galley.text().to_owned()),
                    egui::epaint::Shape::Vec(parts) => {
                        parts.iter().for_each(|part| texts(part, out))
                    }
                    _ => {}
                }
            }
            let mut rendered = vec![];
            for shape in output.shapes {
                texts(&shape.shape, &mut rendered);
            }
            let rendered = rendered.join("\n");
            assert!(rendered.contains("数量積算は未算定です"), "{rendered}");
            assert!(rendered.contains("SecondaryMemberId(539)"), "{rendered}");
            assert!(!rendered.contains("コンクリート[m³]"), "{rendered}");
        }
    }
}

fn primary_column_model() -> Model {
    let source = crate::sample::portal_frame();
    let mut model = Model::with_default_load_cases();
    model.nodes = vec![source.nodes[0].clone(), source.nodes[2].clone()];
    model.nodes[1].id = NodeId(1);
    model.nodes[1].coord = [0.0, 0.0, 3000.0];
    for node in &mut model.nodes {
        node.restraint = sepika_core::dof::Dof6Mask::FIXED;
    }
    let mut concrete = source.materials[0].clone();
    concrete.name = "Fc24".into();
    concrete.category = MaterialCategory::Concrete;
    concrete.young = 25000.0;
    concrete.density = 2.4e-9;
    concrete.fc = Some(24.0);
    concrete.fy = None;
    let mut steel = source.materials[0].clone();
    steel.id = MaterialId(1);
    model.materials = vec![concrete, steel];
    let mut section = shapes()[0]
        .clone()
        .to_section(SectionId(0), "主柱CFT".into());
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = Some(MaterialId(0));
    section.steel_material = Some(MaterialId(1));
    model.sections.push(section);
    let mut element = source.elements[0].clone();
    element.nodes = [NodeId(0), NodeId(1)].into_iter().collect();
    model.elements.push(element);
    model
}

#[test]
fn primary_cft_column_keeps_independent_quantity_and_edit_save_prepare_contract() {
    use sepika_design_jp::quantity::{try_compute_quantity_takeoff, QuantityCfg};
    let mut app = App::default();
    let mut model = primary_column_model();
    let shape = model.sections[0].shape.as_ref().unwrap();
    assert!((model.sections[0].area - 15600.0).abs() < 1e-9);
    assert!((shape.try_cft_core_props().unwrap().unwrap().area - 144400.0).abs() < 1e-9);
    assert!(app.load_model(model.clone()));
    assert!(app.core.scoped.undo.run(
        &mut model,
        Box::new(sepika_edit::EditSectionShape {
            section: SectionId(0),
            new_shape: shapes()[0].clone(),
            frame_use: Some(FrameSectionUse::Column),
        })
    ));
    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    assert_eq!(q.items.len(), 1);
    assert!((q.totals().steel_t - 0.36738).abs() < 1e-9);
    assert!((q.totals().concrete_m3 - 0.4332).abs() < 1e-9);
    model.validate().unwrap();
    assert!(Analysis::prepare(&model).is_ok());
    let path = std::env::temp_dir().join(format!("sepika539-primary-{}.ovika", std::process::id()));
    sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
    let loaded = sepika_io::ovika::load_ovika(&path).unwrap().model;
    assert!(loaded.eq_ignoring_dofmap(&model));
    let q = try_compute_quantity_takeoff(&loaded, &QuantityCfg::default()).unwrap();
    assert!((q.totals().steel_t - 0.36738).abs() < 1e-9);
    assert!((q.totals().concrete_m3 - 0.4332).abs() < 1e-9);
    assert!(Analysis::prepare(&loaded).is_ok());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn rc_steel_src_secondary_members_keep_edit_save_quantity_and_prepare_contract() {
    use sepika_core::section_shape::RcRectColumnRebar;
    use sepika_design_jp::quantity::{try_compute_quantity_takeoff, QuantityCfg};
    let shapes = [
        SectionShape::SteelH {
            height: 400.0,
            width: 200.0,
            web_thick: 8.0,
            flange_thick: 13.0,
            root_r: Some(0.0),
        },
        SectionShape::RcColumnRect {
            b: 400.0,
            d: 400.0,
            rebar: RcRectColumnRebar {
                main_dia: 0.0,
                x: vec![],
                y: vec![],
                cover: 0.0,
                hoop: sepika_core::section_shape::RectColumnHoop {
                    dia: 0.0,
                    pitch: 0.0,
                    legs_x: 0,
                    legs_y: 0,
                },
            },
        },
        SectionShape::SrcColumnRect {
            b: 400.0,
            d: 400.0,
            rebar: RcRectColumnRebar {
                main_dia: 0.0,
                x: vec![],
                y: vec![],
                cover: 0.0,
                hoop: sepika_core::section_shape::RectColumnHoop {
                    dia: 0.0,
                    pitch: 0.0,
                    legs_x: 0,
                    legs_y: 0,
                },
            },
            steel_height: 200.0,
            steel_width: 100.0,
            steel_web_thick: 8.0,
            steel_flange_thick: 10.0,
        },
    ];
    for (idx, shape) in shapes.into_iter().enumerate() {
        for kind in [SecondaryMemberKind::Beam, SecondaryMemberKind::Post] {
            let source = crate::sample::portal_frame();
            let mut model = Model::with_default_load_cases();
            model.sections = source.sections.clone();
            model.materials = source.materials.clone();
            let mut concrete = primary_column_model().materials[0].clone();
            concrete.id = MaterialId(1);
            model.materials.push(concrete);
            let coords = if kind == SecondaryMemberKind::Beam {
                [
                    [0.0, 0.0, 0.0],
                    [6000.0, 0.0, 0.0],
                    [6000.0, 6000.0, 0.0],
                    [0.0, 6000.0, 0.0],
                    [3000.0, 0.0, 0.0],
                    [3000.0, 6000.0, 0.0],
                ]
            } else {
                [
                    [0.0, 0.0, 0.0],
                    [6000.0, 0.0, 0.0],
                    [6000.0, 0.0, 3500.0],
                    [0.0, 0.0, 3500.0],
                    [3000.0, 0.0, 0.0],
                    [3000.0, 0.0, 3500.0],
                ]
            };
            for (i, coord) in coords.into_iter().enumerate() {
                let mut node = source.nodes[0].clone();
                node.id = NodeId(i as u32);
                node.coord = coord;
                node.restraint = sepika_core::dof::Dof6Mask::FIXED;
                model.nodes.push(node);
            }
            for (i, nodes) in [[0, 1], [1, 2], [2, 3], [3, 0]].into_iter().enumerate() {
                let mut element = source.elements[2].clone();
                element.id = ElemId(i as u32);
                element.nodes = nodes.into_iter().map(NodeId).collect();
                element.section = Some(SectionId(
                    if kind == SecondaryMemberKind::Post && i % 2 == 1 {
                        0
                    } else {
                        1
                    },
                ));
                element.local_axis.ref_vector = if kind == SecondaryMemberKind::Post {
                    [0.0, 1.0, 0.0]
                } else {
                    [0.0, 0.0, 1.0]
                };
                model.elements.push(element);
            }
            let boundary = vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)];
            let parent = match kind {
                SecondaryMemberKind::Beam => {
                    model
                        .floor_regions
                        .push(FloorRegion::new(FloorRegionId(0), boundary));
                    sepika_edit::SecondaryParent::Floor(FloorRegionId(0))
                }
                SecondaryMemberKind::Post => {
                    model
                        .wall_regions
                        .push(WallRegion::new(WallRegionId(0), boundary));
                    sepika_edit::SecondaryParent::Wall(WallRegionId(0))
                }
            };
            let mut section = shape.to_section(SectionId(2), "正常二次材".into());
            section.material = Some(MaterialId(if idx == 0 { 0 } else { 1 }));
            if idx == 2 {
                section.steel_material = Some(MaterialId(0));
            }
            model.sections.push(section);
            model.rebuild_floor_assignment_regions();
            model.rebuild_wall_assignment_regions();
            let mut undo = UndoStack::default();
            assert!(undo.run(
                &mut model,
                Box::new(sepika_edit::PlaceSecondaryMember {
                    parent,
                    kind,
                    ends: SecondaryMemberEnds::Supported([
                        SecondaryMemberAnchor {
                            support: SupportMemberId::Primary(ElemId(0)),
                            position: 0.5
                        },
                        SecondaryMemberAnchor {
                            support: SupportMemberId::Primary(ElemId(2)),
                            position: 0.5
                        },
                    ]),
                    section: Some(SectionId(2)),
                    name: "正常二次材".into(),
                })
            ));
            if kind == SecondaryMemberKind::Post {
                model.wall_regions[0].posts[0].gravity_end_shares = Some([1.0, 0.0]);
            }
            for region in &mut model.floor_assignment_regions.regions {
                region.assignment = PlateAssignment::NoPlate;
            }
            for region in &mut model.wall_assignment_regions.regions {
                region.assignment = PlateAssignment::NoPlate;
            }
            model.validate().unwrap();
            Analysis::prepare(&model)
                .unwrap_or_else(|error| panic!("normal shape {idx}, {kind:?}: {error}"));
            let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
            assert_eq!(q.items.len(), 5);
            assert_eq!(
                q.items
                    .iter()
                    .find(|item| item.label == "正常二次材")
                    .unwrap()
                    .structure,
                [
                    sepika_core::structure_kind::StructureKind::S,
                    sepika_core::structure_kind::StructureKind::Rc,
                    sepika_core::structure_kind::StructureKind::Src
                ][idx]
            );
            let path = std::env::temp_dir().join(format!(
                "sepika539-normal-{}-{idx}-{kind:?}.ovika",
                std::process::id()
            ));
            sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
            let loaded = sepika_io::ovika::load_ovika(&path).unwrap().model;
            assert!(loaded.eq_ignoring_dofmap(&model));
            assert_eq!(
                try_compute_quantity_takeoff(&loaded, &QuantityCfg::default())
                    .unwrap()
                    .items
                    .len(),
                5
            );
            assert!(Analysis::prepare(&loaded).is_ok());
            std::fs::remove_file(path).unwrap();
        }
    }
}
