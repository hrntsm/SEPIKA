use super::*;
use sepika_core::ids::StoryId;
use sepika_core::model::*;
use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};
use sepika_load::self_weight::{girder_self_weight_breakdown, self_weight_case_content};

fn model() -> Model {
    let mut model = crate::sample::portal_frame();
    model.nodes.clear();
    model.elements.clear();
    model.load_cases.clear();
    model.stories[1].elevation = Some(3000.0);
    model.stories[1].sl_offset_mm = Some(-350.0);
    model.materials[0].density = 0.0;
    let mut concrete = model.materials[0].clone();
    concrete.id = MaterialId(1);
    concrete.category = MaterialCategory::Concrete;
    concrete.fc = Some(24.0);
    concrete.fy = None;
    concrete.density = 24.0e-6 / sepika_core::units::GRAVITY_MM_S2;
    model.materials.push(concrete);
    model.sections[0].frame_use = Some(FrameSectionUse::Girder);
    model.sections[1] = SectionShape::RcBeamRect {
        b: 400.0,
        d: 700.0,
        rebar: RcBeamRebar {
            main_dia: 25.0,
            top: vec![3],
            bottom: vec![3],
            cover: 50.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 150.0,
                legs: 2,
            },
        },
    }
    .to_section(SectionId(1), "G616".into());
    model.sections[1].frame_use = Some(FrameSectionUse::Girder);
    model.sections[1].material = Some(MaterialId(1));
    for (i, (x, y)) in [
        (0., 0.),
        (6000., 0.),
        (6000., 2000.),
        (0., 2000.),
        (6000., -2000.),
        (0., -2000.),
    ]
    .into_iter()
    .enumerate()
    {
        model.nodes.push(Node {
            id: NodeId(i as u32),
            coord: [x, y, 2650.],
            restraint: sepika_core::dof::Dof6Mask::FIXED,
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    for (i, (a, b)) in [(0, 1), (1, 2), (2, 3), (3, 0), (1, 4), (4, 5), (5, 0)]
        .into_iter()
        .enumerate()
    {
        let mut e = crate::sample::portal_frame().elements[2].clone();
        e.id = ElemId(i as u32);
        e.nodes = [NodeId(a), NodeId(b)].into_iter().collect();
        e.section = Some(SectionId(if i == 0 { 1 } else { 0 }));
        e.vertical_eccentricity = Some(0.0);
        model.elements.push(e);
    }
    let mut ground = model.nodes[0].clone();
    ground.id = NodeId(model.nodes.len() as u32);
    ground.coord[2] = 0.0;
    model.nodes.push(ground);
    for boundary in [[0, 1, 2, 3], [0, 5, 4, 1]] {
        model.add_enclosed_slab_from_nodes(&boundary.map(NodeId), SlabPlate::default());
        model.slabs.pop();
        model
            .floor_assignment_regions
            .regions
            .last_mut()
            .unwrap()
            .assignment = PlateAssignment::NoPlate;
    }
    model
}

fn slab(model: &mut Model, side: usize, thickness: f64, top: f64) {
    let sid = SectionId(model.sections.len() as u32);
    let mut section = SectionShape::RcSlab { thickness }.to_section(sid, "S616".into());
    section.material = Some(MaterialId(1));
    model.sections.push(section);
    let boundary = if side == 0 {
        [0, 1, 2, 3]
    } else {
        [0, 5, 4, 1]
    };
    let id = model.add_enclosed_slab_from_nodes(
        &boundary.map(NodeId),
        SlabPlate {
            section: Some(sid),
            ..Default::default()
        },
    );
    let slab = model.slabs.iter_mut().find(|s| s.id == id).unwrap();
    slab.reference_story = Some(StoryId(1));
    slab.vertical_eccentricity_mm = Some(top);
}

fn weight(model: &Model) -> f64 {
    let w = girder_self_weight_breakdown(model, &model.elements[0])
        .unwrap()
        .unwrap();
    w.body_weight_n - w.slab_overlap_weight_n
}

fn dl_weight(model: &Model) -> f64 {
    let (_, loads) = self_weight_case_content(
        model,
        model.load_cfg.as_ref().unwrap_or(&LoadCfg::default()),
    )
    .unwrap();
    loads
        .iter()
        .filter(|l| l.elem == ElemId(0))
        .map(|l| match l.kind {
            MemberLoadKind::Distributed { a, b, w1, w2 } => (b - a) * (w1 + w2) / 2.,
            _ => panic!(),
        })
        .sum()
}

#[test]
fn girder_overlap_independent_values_reach_preparation_dl_and_seismic_weight() {
    for (top, left, right, expected) in [
        (0., None, None, 40320.),
        (-200., Some((150., 0.)), None, 40320.),
        (-50., Some((150., 0.)), None, 34560.),
        (0., Some((150., 0.)), None, 31680.),
        (0., Some((200., 0.)), None, 28800.),
        (-50., Some((150., 0.)), Some((200., 0.)), 33120.),
        (-50., Some((150., 0.)), Some((150., 400.)), 37440.),
    ] {
        for swap in [false, true] {
            let mut model = model();
            model.elements[0].vertical_eccentricity = Some(top);
            if let Some((t, z)) = left {
                slab(&mut model, usize::from(swap), t, z);
            }
            if let Some((t, z)) = right {
                slab(&mut model, usize::from(!swap), t, z);
            }
            assert!((weight(&model) - expected).abs() < 1e-6);
            assert!((dl_weight(&model) - expected).abs() < 1e-6);
            let generated = sepika_load::story_gen::generate_stories(&model, None).unwrap();
            let total: f64 = generated
                .stories
                .iter()
                .map(|s| s.seismic_weight.unwrap_or(0.))
                .sum();
            assert!((total - expected).abs() < 1e-6, "{total} {expected}");
            let mut app = App::default();
            assert!(app.load_model(model));
            app.run_preparation();
            let prep = app
                .core
                .scoped
                .preparation
                .as_ref()
                .unwrap_or_else(|| panic!("{:?}", app.core.scoped.last_error));
            let row = prep
                .girder_self_weights
                .iter()
                .find(|w| w.element == ElemId(0))
                .unwrap();
            assert!((row.body_weight_n - row.slab_overlap_weight_n - expected).abs() < 1e-6);
            assert!((dl_weight(&app.core.model) - expected).abs() < 1e-6);
            let floor_weight_n =
                288. * (left.map_or(0., |(t, _)| t) + right.map_or(0., |(t, _)| t));
            let dl = prep
                .load_cases
                .iter()
                .find(|c| c.kind == LoadCaseKind::Dead)
                .unwrap();
            assert!(
                (-dl.sum_force[2] - expected - floor_weight_n).abs() < 1e-6,
                "DL {} expected {} + {}",
                -dl.sum_force[2],
                expected,
                floor_weight_n
            );
            assert!(
                (prep.summary.total_seismic_weight - expected - floor_weight_n).abs() < 1e-6,
                "seismic {}",
                prep.summary.total_seismic_weight
            );
            assert!(crate::summary::build_preparation_csv(&app).contains("床板重複控除[kN]"));
        }
    }
}

#[test]
fn girder_overlap_presence_duplicates_and_common_thickness_are_independent() {
    let mut model = model();
    model.elements[0].vertical_eccentricity = Some(-50.);
    slab(&mut model, 0, 150., 0.);
    slab(&mut model, 1, 200., 0.);
    model.floor_assignment_regions.regions[1].assignment = PlateAssignment::NoPlate;
    model.slabs.pop();
    assert!((weight(&model) - 34560.).abs() < 1e-6);
    let mut slab_material = model.materials[1].clone();
    slab_material.id = MaterialId(2);
    slab_material.density /= 2.;
    model.materials.push(slab_material);
    model.sections[2].material = Some(MaterialId(2));
    assert!((weight(&model) - 34560.).abs() < 1e-6);
    for t in [0., 25., 1000., f64::NAN] {
        model.slab_thickness = t;
        assert!((weight(&model) - 34560.).abs() < 1e-6);
    }
    model.slab_thickness = 150.;
    let duplicate = model.floor_assignment_regions.regions[0].clone();
    model.floor_assignment_regions.regions.push(duplicate);
    assert!((weight(&model) - 34560.).abs() < 1e-6);
    model.floor_assignment_regions.regions.pop();
    model.elements[0].vertical_eccentricity = Some(0.);
    model.sections[2].thickness = Some(900.);
    assert!(weight(&model).abs() < 1e-6);
    let mut unrelated = model.elements[0].clone();
    unrelated.id = ElemId(model.elements.len() as u32);
    let n = model.nodes.len() as u32;
    for mut node in [model.nodes[0].clone(), model.nodes[1].clone()] {
        node.id = NodeId(n + node.id.0);
        node.coord[1] += 10000.;
        model.nodes.push(node);
    }
    unrelated.nodes = [NodeId(n), NodeId(n + 1)].into_iter().collect();
    model.elements.push(unrelated);
    let w = girder_self_weight_breakdown(&model, model.elements.last().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(w.slab_overlap_weight_n, 0.);
}

#[test]
fn girder_overlap_invalid_inputs_do_not_become_zero_overlap() {
    let mut base = model();
    slab(&mut base, 0, 150., 0.);
    slab(&mut base, 1, 200., 0.);
    type InvalidInput = Box<dyn Fn(&mut Model)>;
    let invalid: Vec<InvalidInput> = vec![
        Box::new(|m| m.elements[0].section = None),
        Box::new(|m| m.elements[0].section = Some(SectionId(999))),
        Box::new(|m| m.floor_assignment_regions.regions[1].assignment = PlateAssignment::Unset),
        Box::new(|m| {
            m.floor_assignment_regions.regions[1].assignment =
                PlateAssignment::Plate(sepika_core::ids::SlabId(999))
        }),
        Box::new(|m| m.slabs[1].reference_story = None),
        Box::new(|m| m.slabs[1].reference_story = Some(StoryId(999))),
        Box::new(|m| m.slabs[1].vertical_eccentricity_mm = None),
        Box::new(|m| m.slabs[1].vertical_eccentricity_mm = Some(f64::NAN)),
        Box::new(|m| m.slabs[1].plate.section = None),
        Box::new(|m| m.sections[3].thickness = Some(-1.)),
        Box::new(|m| m.sections[3].thickness = Some(f64::INFINITY)),
        Box::new(|m| m.sections[3].material = None),
        Box::new(|m| m.elements[0].vertical_eccentricity = None),
        Box::new(|m| m.nodes[0].coord[0] = f64::NAN),
        Box::new(|m| m.materials[1].density = f64::NAN),
        Box::new(|m| m.materials[1].density = -1.),
        Box::new(|m| m.sections[1].material = None),
        Box::new(|m| m.floor_assignment_regions.regions[0].boundary[0].span = [0.1, 0.9]),
        Box::new(|m| {
            m.floor_assignment_regions.regions[0].boundary[1].support =
                SupportMemberId::Primary(ElemId(999))
        }),
        Box::new(|m| {
            if let Some(SectionShape::RcBeamRect { b, .. }) = &mut m.sections[1].shape {
                *b = -1.;
            }
        }),
    ];
    for (i, edit) in invalid.into_iter().enumerate() {
        let mut model = base.clone();
        edit(&mut model);
        assert!(
            girder_self_weight_breakdown(&model, &model.elements[0]).is_err(),
            "case {i}"
        );
        assert!(
            self_weight_case_content(&model, &LoadCfg::default()).is_err(),
            "DL case {i}"
        );
    }
    let mut conflicting = base.clone();
    conflicting.floor_assignment_regions.regions[1].assignment =
        PlateAssignment::Plate(sepika_core::ids::SlabId(0));
    conflicting.slabs.pop();
    assert!(conflicting.validate_assignment_region_identity().is_err());
    assert!(self_weight_case_content(&conflicting, &LoadCfg::default()).is_err());
    let mut conflicting = base.clone();
    let mut slab = conflicting.slabs[0].clone();
    slab.id = sepika_core::ids::SlabId(2);
    conflicting.slabs.push(slab);
    let mut region = conflicting.floor_assignment_regions.regions[0].clone();
    region.assignment = PlateAssignment::Plate(sepika_core::ids::SlabId(2));
    conflicting.floor_assignment_regions.regions.push(region);
    assert!(
        girder_self_weight_breakdown(&conflicting, &conflicting.elements[0])
            .unwrap_err()
            .contains("矛盾")
    );
}

#[test]
fn girder_overlap_sloped_beam_integrates_plan_length_and_finite_end_faces() {
    let mut model = model();
    model.nodes[1].coord[2] += 4500.;
    slab(&mut model, 0, 150., 0.);
    let w = girder_self_weight_breakdown(&model, &model.elements[0])
        .unwrap()
        .unwrap();
    assert!((w.body_weight_n - 50400.).abs() < 1e-6);
    assert!((w.slab_overlap_weight_n - 1368.).abs() < 1e-6);
    assert!((dl_weight(&model) - 49032.).abs() < 1e-6);
    model.slabs[0].vertical_eccentricity_mm = Some(10000.);
    model.sections[2].thickness = Some(20000.);
    let w = girder_self_weight_breakdown(&model, &model.elements[0])
        .unwrap()
        .unwrap();
    assert!((w.slab_overlap_weight_n - 49518.).abs() < 1e-6);
    model.elements[0].nodes.reverse();
    for region in &mut model.floor_assignment_regions.regions {
        for edge in &mut region.boundary {
            if edge.support == SupportMemberId::Primary(ElemId(0)) {
                edge.span = edge.span.map(|t| 1. - t);
            }
        }
    }
    let reversed = girder_self_weight_breakdown(&model, &model.elements[0])
        .unwrap()
        .unwrap();
    assert!((reversed.slab_overlap_weight_n - w.slab_overlap_weight_n).abs() < 1e-6);
}

#[test]
fn girder_overlap_retains_column_face_interval_and_rejects_attached_supports() {
    let mut model = model();
    slab(&mut model, 0, 150., 0.);
    let mut column_section = model.sections[0].clone();
    column_section.id = SectionId(model.sections.len() as u32);
    column_section.frame_use = Some(FrameSectionUse::Column);
    let section = column_section.id;
    model.sections.push(column_section);
    for end in [0, 1] {
        let mut bottom = model.nodes[end].clone();
        bottom.coord[2] = 0.;
        bottom.id = NodeId(model.nodes.len() as u32);
        let id = bottom.id;
        model.nodes.push(bottom);
        let mut column = model.elements[0].clone();
        column.id = ElemId(model.elements.len() as u32);
        column.nodes = [id, NodeId(end as u32)].into_iter().collect();
        column.section = Some(section);
        column.local_axis.ref_vector = [1., 0., 0.];
        model.elements.push(column);
    }
    let w = girder_self_weight_breakdown(&model, &model.elements[0])
        .unwrap()
        .unwrap();
    assert!((w.body_weight_n - 38304.).abs() < 1e-6);
    assert!((w.body_weight_n - w.slab_overlap_weight_n - 30096.).abs() < 1e-6);
    let mut attached = model.slabs[0].clone();
    attached.id = sepika_core::ids::SlabId(1);
    attached.shape = SlabShape::Attached {
        anchor: RegionAnchor::Line {
            nodes: [NodeId(0), NodeId(1)],
            span: [0.2, 0.8],
            transfer: LoadTransfer::Anchor,
        },
        extent: [1000., 1000.],
    };
    model.slabs.push(attached);
    assert!(girder_self_weight_breakdown(&model, &model.elements[0])
        .unwrap_err()
        .contains("取り付く床板 1"));
    model.slabs.last_mut().unwrap().shape = SlabShape::Attached {
        anchor: RegionAnchor::Line {
            nodes: [NodeId(2), NodeId(3)],
            span: [0., 1.],
            transfer: LoadTransfer::Anchor,
        },
        extent: [1000., 1000.],
    };
    assert!((weight(&model) - 30096.).abs() < 1e-6);
}

#[test]
fn girder_overlap_rejects_rolled_width_but_accepts_local_frame_roundoff() {
    let mut model = model();
    slab(&mut model, 0, 150., 0.);
    for roll in [-0.001, 0.001] {
        model.elements[0].local_axis.ref_vector = [0., roll, 1.];
        assert!(girder_self_weight_breakdown(&model, &model.elements[0])
            .unwrap_err()
            .contains("幅方向が水平でない"));
    }
    model.elements[0].local_axis.ref_vector = [0., 1e-16, 1.];
    assert!((weight(&model) - 31680.).abs() < 1e-6);
}

#[test]
fn damper_total_weight_replaces_girder_body_in_preparation_dl_and_seismic_weight() {
    let mut model = model();
    model.load_cfg = Some(LoadCfg {
        dampers: vec![DamperSpec {
            elem: ElemId(0),
            total_weight: 10000.,
        }],
        ..Default::default()
    });
    model.floor_assignment_regions.regions[0].assignment = PlateAssignment::Unset;
    let (nodal, member) =
        self_weight_case_content(&model, model.load_cfg.as_ref().unwrap()).unwrap();
    assert!((nodal.iter().map(|l| -l.values[2]).sum::<f64>() - 10000.).abs() < 1e-6);
    assert!(member.is_empty());
    let mut app = App::default();
    assert!(app.load_model(model));
    app.run_preparation();
    let prep = app
        .core
        .scoped
        .preparation
        .as_ref()
        .unwrap_or_else(|| panic!("{:?}", app.core.scoped.last_error));
    assert!(prep.girder_self_weights.is_empty());
    assert!((prep.summary.total_seismic_weight - 10000.).abs() < 1e-6);
    let dl = prep
        .load_cases
        .iter()
        .find(|c| c.kind == LoadCaseKind::Dead)
        .unwrap();
    assert!((-dl.sum_force[2] - 10000.).abs() < 1e-6);
}

#[test]
fn girder_overlap_only_applies_to_rectangular_rc_src_girders() {
    let mut base = model();
    slab(&mut base, 0, 150., 0.);
    for usage in [FrameSectionUse::Column, FrameSectionUse::Brace] {
        let mut model = base.clone();
        model.sections[1].frame_use = Some(usage);
        assert!(girder_self_weight_breakdown(&model, &model.elements[0])
            .unwrap()
            .is_none());
        assert!((dl_weight(&model) - 40320.).abs() < 1e-6);
    }
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        let mut model = base.clone();
        model.elements[0].kind = kind;
        assert!((weight(&model) - 31680.).abs() < 1e-6);
    }
    let mut src = base.clone();
    let rebar = src.sections[1]
        .shape
        .as_ref()
        .unwrap()
        .beam_rebar()
        .unwrap()
        .clone();
    src.sections[1].shape = Some(SectionShape::SrcBeamRect {
        b: 400.,
        d: 700.,
        rebar,
        steel_height: 300.,
        steel_width: 150.,
        steel_web_thick: 8.,
        steel_flange_thick: 12.,
    });
    src.sections[1].steel_material = Some(MaterialId(0));
    assert!((weight(&src) - 31680.).abs() < 1e-6);
    assert!((dl_weight(&src) - 31680.).abs() < 1e-6);
    let mut model = base;
    model.elements[0].section = Some(SectionId(0));
    assert!(girder_self_weight_breakdown(&model, &model.elements[0])
        .unwrap()
        .is_none());
}

#[test]
fn girder_overlap_edits_undo_redo_and_save_reload_update_weights_and_invalidate_results() {
    use sepika_edit::*;
    let mut base = model();
    slab(&mut base, 0, 150., 0.);
    let region = base.floor_assignment_regions.regions[0].id;
    let edits: Vec<(Box<dyn EditCommand>, f64)> = vec![
        (
            Box::new(SetBeamVerticalEccentricity {
                member: SupportMemberId::Primary(ElemId(0)),
                value: Some(-50.),
            }),
            34560.,
        ),
        (
            Box::new(SetSlabPlacement {
                id: sepika_core::ids::SlabId(0),
                reference_story: Some(StoryId(1)),
                vertical_eccentricity_mm: Some(100.),
            }),
            37440.,
        ),
        (
            Box::new(EditSectionShape {
                section: SectionId(2),
                new_shape: SectionShape::RcSlab { thickness: 200. },
                frame_use: None,
            }),
            28800.,
        ),
        (
            Box::new(SetMaterialField {
                id: MaterialId(1),
                field: MaterialField::Density,
                value: Some(12.0e-6 / sepika_core::units::GRAVITY_MM_S2),
            }),
            15840.,
        ),
        (Box::new(SetFloorPlateRegionNoPlate { region }), 40320.),
        (
            Box::new(SetNodeCoord {
                node: NodeId(1),
                coord: [3000., 0., 2650.],
            }),
            15840.,
        ),
    ];
    for (i, (edit, expected)) in edits.into_iter().enumerate() {
        let mut app = App::default();
        assert!(app.load_model(base.clone()));
        app.run_preparation();
        assert!(app.core.scoped.preparation.is_some());
        app.core.scoped.staleness.mark_fresh();
        app.core.scoped.staleness.design_stale = false;
        let input = app.result_input(&result_validity::ResultInputKey::Modal);
        assert!(app.apply_model_edit(edit), "edit {i}");
        assert!(
            app.core.scoped.staleness.preparation_stale
                && app.core.scoped.staleness.results_stale
                && app.core.scoped.staleness.design_stale
        );
        assert_ne!(
            app.result_input(&result_validity::ResultInputKey::Modal),
            input
        );
        assert!(
            (weight(&app.core.model) - expected).abs() < 1e-6,
            "edit {i}"
        );
        app.undo_action();
        assert!((weight(&app.core.model) - 31680.).abs() < 1e-6, "undo {i}");
        app.redo_action();
        assert!(
            (weight(&app.core.model) - expected).abs() < 1e-6,
            "redo {i}"
        );
        app.run_preparation();
        let prep = app
            .core
            .scoped
            .preparation
            .as_ref()
            .unwrap_or_else(|| panic!("edit {i}: {:?}", app.core.scoped.last_error));
        let row = prep
            .girder_self_weights
            .iter()
            .find(|r| r.element == ElemId(0))
            .unwrap();
        assert!((row.body_weight_n - row.slab_overlap_weight_n - expected).abs() < 1e-6);
        let path =
            std::env::temp_dir().join(format!("sepika-616-{i}-{}.ovika", std::process::id()));
        sepika_io::ovika::save_ovika(&path, &app.core.model, Default::default()).unwrap();
        let restored = sepika_io::ovika::load_ovika(&path).unwrap().model;
        assert!((weight(&restored) - expected).abs() < 1e-6, "reload {i}");
        std::fs::remove_file(path).unwrap();
    }
    let mut app = App::default();
    assert!(app.load_model(base));
    app.run_preparation();
    assert!(app.apply_model_edit(Box::new(UnsetFloorPlateRegion { region })));
    app.run_preparation();
    assert!(app.core.scoped.preparation.is_none());
    assert!(app.core.scoped.last_error.is_some());
    assert!(!sepika_job::weight_preparation::weights_are_current(
        &app.core.model,
        app.core.model.mass_method
    ));
    assert!(app
        .core
        .model
        .seismic_weight_generation
        .as_ref()
        .unwrap()
        .calculated_weights
        .is_empty());
}
