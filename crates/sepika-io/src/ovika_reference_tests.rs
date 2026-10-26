use super::*;
use sepika_core::{ids::*, model::*};

fn section(id: u32, name: &str) -> Section {
    Section::zero(SectionId(id), name.into())
}

fn member_model(place: usize, sections: Vec<Section>, reference: Option<u32>) -> Model {
    let mut model = Model {
        sections,
        ..Default::default()
    };
    let member = SecondaryMember {
        id: SecondaryMemberId(674),
        kind: if matches!(place, 2 | 4) {
            SecondaryMemberKind::Post
        } else {
            SecondaryMemberKind::Beam
        },
        section: reference.map(SectionId),
        ends: SecondaryMemberEnds::Detached([[0., 0., 0.], [1000., 0., 0.]]),
        ..Default::default()
    };
    match place {
        0 => {
            model.nodes = (0..2)
                .map(|id| Node {
                    id: NodeId(id),
                    coord: [1000. * id as f64, 0., 0.],
                    restraint: Default::default(),
                    mass: None,
                    story: None,
                    support_spring: None,
                })
                .collect();
            model.elements.push(ElementData {
                id: ElemId(0),
                kind: ElementKind::Beam,
                nodes: vec![NodeId(0), NodeId(1)].into(),
                section: reference.map(SectionId),
                local_axis: LocalAxis {
                    ref_vector: [0., 0., 1.],
                },
                end_cond: [EndCondition::Fixed; 2],
                force_regime: ForceRegime::Auto,
                rigid_zone: Default::default(),
                vertical_eccentricity: Some(0.0),
                plastic_zone: None,
                spring: None,
            });
        }
        1 => {
            let mut region = FloorRegion::new(FloorRegionId(0), vec![]);
            region.secondary_beams.push(member);
            model.floor_regions.push(region);
        }
        2 => {
            let mut region = WallRegion::new(WallRegionId(0), vec![]);
            region.posts.push(member);
            model.wall_regions.push(region);
        }
        3 => model.unassigned_beams.push(member),
        4 => model.unassigned_posts.push(member),
        _ => unreachable!(),
    }
    model
}

fn correct_hash_file(path: &Path, model: &Model) {
    let bytes = rmp_serde::to_vec_named(model).unwrap();
    let manifest = Manifest {
        schema_version: CURRENT_SCHEMA_VERSION,
        units: "internal: N-mm-s".into(),
        created_by: "reference-contract-fixture".into(),
        entries: vec![crate::manifest::EntryHash {
            name: "model.msgpack".into(),
            sha256: sha256_of(&bytes),
        }],
    };
    let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let options = zip::write::FileOptions::<()>::default();
    for (name, data) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("model.msgpack", bytes),
    ] {
        writer.start_file(name, options).unwrap();
        writer.write_all(&data).unwrap();
    }
    writer.finish().unwrap();
}

fn assert_rejected_preserving_input(model: &Model, label: &str, reason: &str) {
    let path = crate::test_util::test_tmp().join(format!("reference-{label}.ovika"));
    save_ovika(
        &path,
        &Model::default(),
        OvikaExtras {
            results: Some(b"old-results"),
            ..Default::default()
        },
    )
    .unwrap();
    let file = std::fs::read(&path).unwrap();
    let before = rmp_serde::to_vec_named(model).unwrap();
    let error = save_ovika(&path, model, OvikaExtras::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains(reason), "{label}: {error}");
    assert_eq!(std::fs::read(&path).unwrap(), file, "{label}");
    let baseline = load_ovika(&path).unwrap();
    assert!(baseline.model.eq_ignoring_dofmap(&Model::default()));
    assert_eq!(baseline.results.as_deref(), Some(b"old-results".as_slice()));
    let raw = path.with_extension("raw.ovika");
    correct_hash_file(&raw, model);
    let bytes = std::fs::read(&raw).unwrap();
    let error = load_ovika(&raw).err().unwrap().to_string();
    assert!(error.contains(reason), "{label}: {error}");
    assert_eq!(std::fs::read(&raw).unwrap(), bytes, "{label}");
    assert_eq!(rmp_serde::to_vec_named(model).unwrap(), before, "{label}");
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(raw).unwrap();
}

fn assert_roundtrip(model: &Model, label: &str) {
    let path = crate::test_util::test_tmp().join(format!("reference-{label}.ovika"));
    let before = rmp_serde::to_vec_named(model).unwrap();
    save_ovika(
        &path,
        model,
        OvikaExtras {
            preparation: Some(b"preparation"),
            results: Some(b"results"),
            analysis_settings: Some(b"settings"),
        },
    )
    .unwrap();
    let contents = load_ovika(&path).unwrap();
    assert!(contents.model.eq_ignoring_dofmap(model), "{label}");
    assert_eq!(
        contents.preparation.as_deref(),
        Some(b"preparation".as_slice())
    );
    assert_eq!(contents.results.as_deref(), Some(b"results".as_slice()));
    assert_eq!(
        contents.analysis_settings.as_deref(),
        Some(b"settings".as_slice())
    );
    assert_eq!(rmp_serde::to_vec_named(model).unwrap(), before);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn all_member_locations_reject_noncanonical_section_references_and_keep_incomplete_models() {
    for place in 0..5 {
        for (name, sections, reference, accepted) in [
            ("canonical", vec![section(0, "正規")], Some(0), true),
            ("unset", vec![], None, true),
            (
                "out-of-range-existing",
                vec![section(7, "範囲外")],
                Some(7),
                false,
            ),
            ("missing", vec![section(0, "正規")], Some(7), false),
            (
                "mismatch",
                vec![section(1, "参照先"), section(9, "別断面")],
                Some(1),
                false,
            ),
            (
                "duplicate-canonical",
                vec![section(1, "先頭"), section(1, "添字一致")],
                Some(1),
                false,
            ),
            (
                "duplicate-fallback",
                vec![section(7, "先頭"), section(7, "後続")],
                Some(7),
                false,
            ),
        ] {
            let model = member_model(place, sections, reference);
            let label = format!("member-{place}-{name}");
            if accepted {
                assert!(model.validate_reference_ids().is_ok(), "{label}");
                if place == 0 && reference.is_some() {
                    assert!(model
                        .validate()
                        .unwrap_err()
                        .to_string()
                        .contains("断面用途が未設定"));
                }
                assert_roundtrip(&model, &label);
            } else {
                assert_rejected_preserving_input(&model, &label, "Section");
            }
        }
    }
    let mut duplicate = member_model(1, vec![], None);
    duplicate.unassigned_posts.push(SecondaryMember {
        id: SecondaryMemberId(674),
        kind: SecondaryMemberKind::Post,
        ends: SecondaryMemberEnds::Detached([[0.; 3], [0., 0., 1000.]]),
        ..Default::default()
    });
    assert_rejected_preserving_input(&duplicate, "secondary-duplicate", "安定 ID が重複");
}

fn reference_model() -> Model {
    let mut model = member_model(0, vec![section(0, "直接入力")], Some(0));
    model.materials.push(Material {
        id: MaterialId(0),
        name: "材料".into(),
        category: MaterialCategory::Steel,
        young: 205000.,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: None,
        concrete_class: Default::default(),
        strength_factor: None,
    });
    model.stories.push(Story {
        id: StoryId(0),
        name: "階".into(),
        elevation: Some(0.),
        sl_offset_mm: Some(0.0),
        source_story: None,
        node_ids: vec![],
        seismic_weight: None,
        weight_override: None,
        structure: Default::default(),
        level_kind: Default::default(),
        dynamic_mass: None,
        standard_floor_load: None,
        column_finish_area_weight: 0.,
        fireproof: Default::default(),
        wall_weights: vec![],
    });
    model.floor_regions.push(FloorRegion::new(
        FloorRegionId(0),
        vec![NodeId(0), NodeId(1)],
    ));
    model
        .wall_regions
        .push(WallRegion::new(WallRegionId(0), vec![NodeId(0), NodeId(1)]));
    model.slabs.push(Slab {
        id: SlabId(0),
        shape: SlabShape::Enclosed,
        plate: SlabPlate::default(),
        reference_story: None,
        vertical_eccentricity_mm: None,
        tip_loads: vec![],
    });
    model.wall_plates.push(WallPlate {
        id: WallPlateId(0),
        shape: WallPlateShape::Enclosed,
        section: None,
        opening_area: 0.,
        opening_weight: 0.,
        openings: vec![],
        loads: vec![],
        slit: Default::default(),
        self_weight_shares: vec![],
        dl_support: None,
    });
    model.axes.push(AxisGroup {
        name: "通り".into(),
        kind: AxisGroupKind::Other,
        axes: vec![Axis {
            name: "1".into(),
            distance: None,
            nodes: vec![NodeId(0)],
            source: AxisSource::Manual,
        }],
    });
    model.sections[0].material = Some(MaterialId(0));
    model.sections[0].rebar_material = Some(MaterialId(0));
    model.sections[0].shear_rebar_material = Some(MaterialId(0));
    model.sections[0].steel_material = Some(MaterialId(0));
    model.floor_regions[0].slab_ids.push(SlabId(0));
    model.wall_regions[0].wall_plate_ids.push(WallPlateId(0));
    model.slabs[0].plate.section = Some(SectionId(0));
    model.wall_plates[0].section = Some(SectionId(0));
    model.girder_groups.push(vec![ElemId(0)]);
    model
}

#[test]
fn indexed_ids_and_existing_reference_families_are_checked_before_persistence() {
    type Mutation = (&'static str, fn(&mut Model), &'static str);
    let mutations: &[Mutation] = &[
        ("node-id", |m| m.nodes[0].id = NodeId(7), "NodeId(7)"),
        (
            "element-id",
            |m| m.elements[0].id = ElemId(674),
            "ElemId(674)",
        ),
        (
            "section-id",
            |m| {
                m.elements[0].section = None;
                m.slabs[0].plate.section = None;
                m.wall_plates[0].section = None;
                m.sections[0].id = SectionId(7)
            },
            "SectionId(7)",
        ),
        (
            "material-id",
            |m| {
                m.sections[0].material = None;
                m.sections[0].rebar_material = None;
                m.sections[0].shear_rebar_material = None;
                m.sections[0].steel_material = None;
                m.materials[0].id = MaterialId(7);
            },
            "MaterialId(7)",
        ),
        ("story-id", |m| m.stories[0].id = StoryId(7), "StoryId(7)"),
        (
            "floor-id",
            |m| m.floor_regions[0].id = FloorRegionId(7),
            "FloorRegionId(7)",
        ),
        (
            "wall-id",
            |m| m.wall_regions[0].id = WallRegionId(7),
            "WallRegionId(7)",
        ),
        (
            "slab-id",
            |m| {
                m.floor_regions[0].slab_ids.clear();
                m.slabs[0].id = SlabId(7);
            },
            "SlabId(7)",
        ),
        (
            "plate-id",
            |m| m.wall_plates[0].id = WallPlateId(7),
            "WallPlateId(7)",
        ),
        (
            "element-node",
            |m| m.elements[0].nodes[1] = NodeId(7),
            "Elem 0 -> Node 7",
        ),
        (
            "material",
            |m| m.sections[0].material = Some(MaterialId(7)),
            "Section 0 -> Material 7",
        ),
        (
            "rebar-material",
            |m| m.sections[0].rebar_material = Some(MaterialId(7)),
            "Section 0 -> RebarMaterial 7",
        ),
        (
            "shear-material",
            |m| m.sections[0].shear_rebar_material = Some(MaterialId(7)),
            "Section 0 -> ShearRebarMaterial 7",
        ),
        (
            "steel-material",
            |m| m.sections[0].steel_material = Some(MaterialId(7)),
            "Section 0 -> SteelMaterial 7",
        ),
        (
            "axis-node",
            |m| m.axes[0].axes[0].nodes[0] = NodeId(7),
            "Axis 通り/1 -> Node 7",
        ),
        (
            "floor-node",
            |m| m.floor_regions[0].boundary[0] = NodeId(7),
            "FloorRegion 0 -> Node 7",
        ),
        (
            "wall-node",
            |m| m.wall_regions[0].boundary[0] = NodeId(7),
            "WallRegion 0 -> Node 7",
        ),
        (
            "floor-slab",
            |m| m.floor_regions[0].slab_ids.push(SlabId(7)),
            "FloorRegion 0 -> Slab 7",
        ),
        (
            "wall-plate",
            |m| m.wall_regions[0].wall_plate_ids.push(WallPlateId(7)),
            "WallRegion 0 -> WallPlate 7",
        ),
        (
            "slab-section",
            |m| m.slabs[0].plate.section = Some(SectionId(7)),
            "Slab 0 -> Section 7",
        ),
        (
            "plate-section",
            |m| m.wall_plates[0].section = Some(SectionId(7)),
            "WallPlate 0 -> Section 7",
        ),
        (
            "slab-node",
            |m| {
                m.slabs[0].shape = SlabShape::Attached {
                    anchor: RegionAnchor::Point(NodeId(7)),
                    extent: [1000.; 2],
                }
            },
            "Slab 0 -> Node 7",
        ),
        (
            "plate-node",
            |m| {
                m.wall_plates[0].shape = WallPlateShape::Attached {
                    anchor: RegionAnchor::FloorRegion {
                        nodes: [NodeId(0), NodeId(7)],
                    },
                    extent: None,
                }
            },
            "WallPlate 0 -> Node 7",
        ),
        (
            "girder-element",
            |m| m.girder_groups[0][0] = ElemId(7),
            "GirderGroup 0 -> Elem 7",
        ),
    ];
    let normal = reference_model();
    assert_roundtrip(&normal, "reference-families-normal");
    for (label, mutate, reason) in mutations {
        let mut model = normal.clone();
        mutate(&mut model);
        assert_rejected_preserving_input(&model, label, reason);
    }
}

#[test]
fn built_box_ovika_roundtrip_keeps_both_thicknesses_pending_properties_and_references() {
    use sepika_core::section_shape::SectionShape;
    for cft in [false, true] {
        let shape = if cft {
            SectionShape::CftBox {
                height: 300.0,
                width: 200.0,
                thick: 9.0,
                horizontal_thick: 12.0,
                corner_r: Some(0.0),
            }
        } else {
            SectionShape::SteelBox {
                height: 300.0,
                width: 200.0,
                thick: 9.0,
                horizontal_thick: 12.0,
                corner_r: Some(0.0),
            }
        };
        let mut s = shape.input_section(SectionId(0), "異厚BOX".into()).unwrap();
        s.frame_use = Some(FrameSectionUse::Column);
        for place in 0..if cft { 1 } else { 5 } {
            let mut model = member_model(place, vec![s.clone(), section(1, "別断面")], Some(0));
            model.visit_section_ids(|id| id.0 = 1 - id.0);
            model.sections.reverse();
            assert_eq!(
                model.sections[1].shape.as_ref().unwrap().box_thicknesses(),
                Some((9.0, 12.0))
            );
            assert_roundtrip(&model, &format!("built-box-{cft}-{place}"));
            assert!(crate::stbridge::export_stbridge(&model)
                .unwrap_err()
                .to_string()
                .contains("異厚BOXのSTB出力は未対応"));
            assert_eq!(
                (
                    model.sections[1].area,
                    model.sections[1].iy,
                    model.sections[1].iz
                ),
                (9768.0, 131127264.0, 61342936.0)
            );
        }
    }
}

#[test]
fn misc889_ovika_rejects_invalid_n_without_replacing_saved_model() {
    let path = crate::test_util::test_tmp().join("misc889-n.ovika");
    let mut model = Model::default();
    for n in [None, Some(1.0), Some(2.0)] {
        model.stress_cfg.misc_wall_n = n;
        save_ovika(&path, &model, OvikaExtras::default()).unwrap();
        assert_eq!(load_ovika(&path).unwrap().model.stress_cfg.misc_wall_n, n);
    }
    let saved = std::fs::read(&path).unwrap();
    for n in [0.5, 0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        model.stress_cfg.misc_wall_n = Some(n);
        assert!(save_ovika(&path, &model, OvikaExtras::default())
            .unwrap_err()
            .to_string()
            .contains("係数n"));
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        assert_eq!(model.stress_cfg.misc_wall_n.unwrap().to_bits(), n.to_bits());
    }
    let mut old_model = serde_json::to_value(Model::default()).unwrap();
    old_model["stress_cfg"]["misc_wall_n"] = 0.5.into();
    let bytes = rmp_serde::to_vec_named(&old_model).unwrap();
    let manifest = Manifest {
        schema_version: CURRENT_SCHEMA_VERSION,
        units: "internal: N-mm-s".into(),
        created_by: "misc889-fixture".into(),
        entries: vec![crate::manifest::EntryHash {
            name: "model.msgpack".into(),
            sha256: sha256_of(&bytes),
        }],
    };
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options = zip::write::FileOptions::<()>::default();
    for (name, data) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("model.msgpack", bytes),
    ] {
        writer.start_file(name, options).unwrap();
        writer.write_all(&data).unwrap();
    }
    writer.finish().unwrap();
    let old_file = std::fs::read(&path).unwrap();
    assert!(load_ovika(&path)
        .err()
        .unwrap()
        .to_string()
        .contains("係数n"));
    assert_eq!(std::fs::read(&path).unwrap(), old_file);
}
